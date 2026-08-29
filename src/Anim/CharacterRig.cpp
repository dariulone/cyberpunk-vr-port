// CharacterRig -- the solve for the PLAYER'S OWN bones.
//
// Forward kinematics, the two-bone arm and leg solves, the hand target and its stop, the torso
// damping and girdle pinning, the arm scaling that fits the avatar to a measured user, and the body
// placement under the HMD. Every function here answers the same question: given a target in some
// space, what local rotation does this character bone take?
//
// It does NOT contain the WEAPON bones. Those are written inside the pose-apply detour, at brace
// depth three to seven and inside its single 1,980-line __try -- see the note at the top of
// src/Hooks/AnimPose.cpp for why they have not been lifted out yet and what it would take.

#include "Anim/VrikHook.hpp"
#include "Anim/CharacterRig.hpp"
// The elbow-policy flag lives with the other live controls.
#include "Camera/CameraState.hpp"
#include "Core/VrCoreShared.hpp"
#include "Runtimes/OpenXRManager.hpp"


// Rotate vector v by quaternion q (q = i,j,k,r == x,y,z,w). o = q * v * q^-1.
void VRIK_QuatRotateVec(const float* q, const float* v, float* o) {
    const float tx = 2.0f * (q[1] * v[2] - q[2] * v[1]);
    const float ty = 2.0f * (q[2] * v[0] - q[0] * v[2]);
    const float tz = 2.0f * (q[0] * v[1] - q[1] * v[0]);
    o[0] = v[0] + q[3] * tx + (q[1] * tz - q[2] * ty);
    o[1] = v[1] + q[3] * ty + (q[2] * tx - q[0] * tz);
    o[2] = v[2] + q[3] * tz + (q[0] * ty - q[1] * tx);
}

// Hamilton product o = a * b (both i,j,k,r == x,y,z,w).
void VRIK_QuatMul(const float* a, const float* b, float* o) {
    o[0] = a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1];
    o[1] = a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0];
    o[2] = a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3];
    o[3] = a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2];
}

// Conjugate (== inverse for a unit quaternion).
void VRIK_QuatConj(const float* q, float* o) {
    o[0] = -q[0]; o[1] = -q[1]; o[2] = -q[2]; o[3] = q[3];
}

void VRIK_QuatNorm(float* q) {
    float n = std::sqrt(q[0]*q[0] + q[1]*q[1] + q[2]*q[2] + q[3]*q[3]);
    if (n > 1e-8f) { float inv = 1.0f / n; q[0]*=inv; q[1]*=inv; q[2]*=inv; q[3]*=inv; }
    else { q[0]=0.0f; q[1]=0.0f; q[2]=0.0f; q[3]=1.0f; }
}

float VRIK_Dot3(const float* a, const float* b) {
    return a[0]*b[0] + a[1]*b[1] + a[2]*b[2];
}
void VRIK_Cross3(const float* a, const float* b, float* o) {
    o[0] = a[1]*b[2] - a[2]*b[1];
    o[1] = a[2]*b[0] - a[0]*b[2];
    o[2] = a[0]*b[1] - a[1]*b[0];
}
float VRIK_Norm3(float* v) {
    float n = std::sqrt(v[0]*v[0] + v[1]*v[1] + v[2]*v[2]);
    if (n > 1e-8f) { float inv = 1.0f/n; v[0]*=inv; v[1]*=inv; v[2]*=inv; }
    return n;
}
static inline float VRIK_Dist3(const float* a, const float* b) {
    float dx=a[0]-b[0], dy=a[1]-b[1], dz=a[2]-b[2];
    return std::sqrt(dx*dx + dy*dy + dz*dz);
}

// slerp(identity, q, t): a fraction t of the rotation q (used to distribute a spine bend
// across several bones so the curve is gradual instead of a single sharp joint).
void VRIK_QuatScale(const float* q, float t, float* o) {
    float qq[4] = { q[0], q[1], q[2], q[3] };
    if (qq[3] < 0.0f) { qq[0]=-qq[0]; qq[1]=-qq[1]; qq[2]=-qq[2]; qq[3]=-qq[3]; } // shortest arc
    float s = std::sqrt(qq[0]*qq[0] + qq[1]*qq[1] + qq[2]*qq[2]);
    if (s < 1e-6f) { o[0]=0.0f; o[1]=0.0f; o[2]=0.0f; o[3]=1.0f; return; } // ~no rotation
    float w = qq[3]; if (w > 1.0f) w = 1.0f;
    float half = std::acos(w) * t;     // scaled half-angle
    float sn = std::sin(half), cn = std::cos(half);
    o[0] = qq[0]/s*sn; o[1] = qq[1]/s*sn; o[2] = qq[2]/s*sn; o[3] = cn;
}

// Shortest-arc unit quaternion rotating unit vector a onto unit vector b.
void VRIK_QuatFromTo(const float* a, const float* b, float* o) {
    float d = VRIK_Dot3(a, b);
    if (d >= 1.0f - 1e-6f) { o[0]=0.0f; o[1]=0.0f; o[2]=0.0f; o[3]=1.0f; return; }
    if (d <= -1.0f + 1e-6f) {
        // Antiparallel: rotate 180 deg about any axis orthogonal to a.
        float ax[3] = { 1.0f, 0.0f, 0.0f };
        if (std::fabs(a[0]) > 0.9f) { ax[0]=0.0f; ax[1]=1.0f; ax[2]=0.0f; }
        float axis[3]; VRIK_Cross3(a, ax, axis); VRIK_Norm3(axis);
        o[0]=axis[0]; o[1]=axis[1]; o[2]=axis[2]; o[3]=0.0f;
        return;
    }
    float c[3]; VRIK_Cross3(a, b, c);
    o[0]=c[0]; o[1]=c[1]; o[2]=c[2]; o[3]=1.0f + d;
    VRIK_QuatNorm(o);
}

// Writes one VR hand into the destination bone buffer (48-byte QsTransform:
// rotation/quaternion @ +0, translation @ +16, scale @ +32 -- confirmed via IDA
// current pose-apply implementation). When head-relative is active the controller's
// head-local offset is rotated by the head bone's orientation and added to the
// head bone's position, so the result lands in the same buffer space as the head.
// This mirrors the working CET gizmo (worldPos = camPos + camQuat * localPos) and
// is what stops the hand from swinging when the head turns.
void VRIK_WriteHand(uint8_t* boneBuf, int bIdx,
                                  const float* headPos, const float* headQuat, bool headOk,
                                  const float* vrPos, const float* vrQuat, bool writeRot) {
    if (bIdx < 0) return;

    const float s = g_VRBindScale;
    float local[3];
    VRIK_RemapAxis(g_VRBindAxis, vrPos, local);
    local[0] *= s; local[1] *= s; local[2] *= s;

    float pos[3];
    if (g_VRUseHeadRelative && headOk) {
        float rotated[3];
        VRIK_QuatRotateVec(headQuat, local, rotated);
        pos[0] = headPos[0] + rotated[0];
        pos[1] = headPos[1] + rotated[1];
        pos[2] = headPos[2] + rotated[2];
    } else {
        pos[0] = local[0];
        pos[1] = local[1];
        pos[2] = local[2];
    }
    pos[0] += g_VRBindOffX; pos[1] += g_VRBindOffY; pos[2] += g_VRBindOffZ;

    // Translation @ +0 (QsTransform), Rotation @ +16 -- see file header.
    float* t = reinterpret_cast<float*>(boneBuf + bIdx * 48 + 0);
    t[0] = pos[0]; t[1] = pos[1]; t[2] = pos[2];

    if (writeRot) {
        // VR->game axis swap, same as the gizmo's mapLocalQuat: (i, -k, j, r).
        float localQuat[4] = { vrQuat[0], -vrQuat[2], vrQuat[1], vrQuat[3] };
        float* r = reinterpret_cast<float*>(boneBuf + bIdx * 48 + 16);
        if (g_VRUseHeadRelative && headOk) {
            VRIK_QuatMul(headQuat, localQuat, r);
        } else {
            r[0] = localQuat[0]; r[1] = localQuat[1]; r[2] = localQuat[2]; r[3] = localQuat[3];
        }
    }
}

// QsTransform field offsets inside each 48-byte bone slot.

// Model-space FK scratch (recomputed each matched frame). Sized generously.
float g_fkPos[VRIK_MAX_BONES][3];
float g_fkRot[VRIK_MAX_BONES][4];

// FK SNAPSHOT -- the measurement channel for capsule geometry.
//
// Scripts could only see eleven hand-picked bones (spine and legs), and the arms were not among them, so
// "does the forearm capsule match the forearm" and "where do the fingers end" were unanswerable without a
// plugin rebuild each time. This copies the whole solved rig once per pose pass so any bone can be measured
// by index; DumpPlayerBoneNames() writes the index table.
//
// Filled only while diag capture is on, which is also the only time the post-solve FK is computed at all --
// on normal frames the arms in g_fkPos are still the ENGINE'S ANIMATED pose, the exact trap that once put
// the ball at the player's hip. Count is published LAST so a reader either sees a complete snapshot or the
// previous one.
volatile float g_VRFKSnapPos[VRIK_MAX_BONES][3];
volatile float g_VRFKSnapRot[VRIK_MAX_BONES][4];
volatile int   g_VRFKSnapCount = 0;

// FK walk length: only the prefix of the rig the solver actually reads
// (parents precede children, so the prefix is self-contained). Falls back to
// the full bone count until the resolve publishes the prefix.
int VRIK_FKCount() {
    const int n = g_VRFKCount;
    return (n > 0) ? n : g_VRBoneCount;
}

// Forward kinematics: accumulate parent-local transforms into model space.
// Requires parent index < child index (true for these rigs / topological order).
void VRIK_ComputeFK(uint8_t* boneBuf, int count) {
    if (count > VRIK_MAX_BONES) count = VRIK_MAX_BONES;
    for (int i = 0; i < count; ++i) {
        const float* lt = reinterpret_cast<float*>(boneBuf + i * 48 + VRIK_TRANS_OFF);
        const float* lr = reinterpret_cast<float*>(boneBuf + i * 48 + VRIK_ROT_OFF);
        float lpos[3] = { lt[0], lt[1], lt[2] };
        float lrot[4] = { lr[0], lr[1], lr[2], lr[3] };
        int p = g_VRBoneParent[i];
        if (p >= 0 && p < i) {
            VRIK_QuatMul(g_fkRot[p], lrot, g_fkRot[i]);
            VRIK_QuatNorm(g_fkRot[i]);
            float rp[3];
            VRIK_QuatRotateVec(g_fkRot[p], lpos, rp);
            g_fkPos[i][0] = g_fkPos[p][0] + rp[0];
            g_fkPos[i][1] = g_fkPos[p][1] + rp[1];
            g_fkPos[i][2] = g_fkPos[p][2] + rp[2];
        } else {
            g_fkRot[i][0]=lrot[0]; g_fkRot[i][1]=lrot[1]; g_fkRot[i][2]=lrot[2]; g_fkRot[i][3]=lrot[3];
            VRIK_QuatNorm(g_fkRot[i]);
            g_fkPos[i][0]=lpos[0]; g_fkPos[i][1]=lpos[1]; g_fkPos[i][2]=lpos[2];
        }
    }
}

// Writes a model-space rotation back into a bone as a LOCAL rotation, given the
// (already updated) model rotation of its parent. localRot = parentModel^-1 * modelRot.
void VRIK_WriteLocalRot(uint8_t* boneBuf, int idx,
                                      const float* parentModelRot, const float* modelRot) {
    float pInv[4]; VRIK_QuatConj(parentModelRot, pInv);
    float local[4]; VRIK_QuatMul(pInv, modelRot, local);
    VRIK_QuatNorm(local);
    float* r = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_ROT_OFF);
    r[0]=local[0]; r[1]=local[1]; r[2]=local[2]; r[3]=local[3];
}

void VRIK_WriteLocalPos(uint8_t* boneBuf, int idx,
                                      const float* parentModelPos, const float* parentModelRot,
                                      const float* modelPos) {
    float delta[3] = {
        modelPos[0] - parentModelPos[0],
        modelPos[1] - parentModelPos[1],
        modelPos[2] - parentModelPos[2]
    };
    float pInv[4]; VRIK_QuatConj(parentModelRot, pInv);
    float local[3]; VRIK_QuatRotateVec(pInv, delta, local);
    float* t = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_TRANS_OFF);
    t[0]=local[0]; t[1]=local[1]; t[2]=local[2];
}

void VRIK_DampenTorsoWeaponPose(uint8_t* boneBuf) {
    // Weapon-ready upper-body poses bend the Spine* chain before VRIK runs. Neutralize only spine
    // local rotations here; clavicle/upper-arm identity is not the rig rest pose and corrupts FK.
    auto neutralize = [&](int idx) {
        if (idx < 0 || idx >= VRIK_MAX_BONES) return;
        float* q = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_ROT_OFF);
        q[0] = 0.0f; q[1] = 0.0f; q[2] = 0.0f; q[3] = 1.0f;
    };

    int count = static_cast<int>(g_VRSpineCount);
    if (count > 0 && count <= 8) {
        for (int i = 0; i < count; ++i) neutralize(g_VRSpineIdx[i]);
    }

    // HIPS LOCK. Strafe/run locomotion rotates the PELVIS ("поворачивается туловище при
    // стрейфе") -- the spine chain above is neutralized relative to the hips, so a hips
    // twist turns the ENTIRE torso incl. the clavicle pivots and the arms drift with it.
    // The identity quat is NOT the rig rest pose for the hips (root convention differs),
    // so capture the live local rotation over the first ~90 solves (idle stance) and pin
    // it afterwards. Legs are re-solved by the leg IK below the hips either way.
    {
        const int hips = g_VRHipsIdx;
        if (hips >= 0 && hips < VRIK_MAX_BONES) {
            float* hq = reinterpret_cast<float*>(boneBuf + hips * 48 + VRIK_ROT_OFF);
            static float s_hipsRef[4] = { 0, 0, 0, 1 };
            static int   s_hipsN = 0;
            static int   s_hipsGen = -1;
            if (s_hipsGen != g_VRPoseCapGen) { s_hipsGen = g_VRPoseCapGen; s_hipsN = 0; }
            if (s_hipsN < 90) {
                if (s_hipsN == 0) {
                    s_hipsRef[0]=hq[0]; s_hipsRef[1]=hq[1]; s_hipsRef[2]=hq[2]; s_hipsRef[3]=hq[3];
                } else {
                    // Incremental average with hemisphere alignment, renormalized.
                    float d = hq[0]*s_hipsRef[0] + hq[1]*s_hipsRef[1] + hq[2]*s_hipsRef[2] + hq[3]*s_hipsRef[3];
                    const float sgn = (d >= 0.0f) ? 1.0f : -1.0f;
                    const float k = 1.0f / static_cast<float>(s_hipsN + 1);
                    s_hipsRef[0] += (sgn*hq[0] - s_hipsRef[0]) * k;
                    s_hipsRef[1] += (sgn*hq[1] - s_hipsRef[1]) * k;
                    s_hipsRef[2] += (sgn*hq[2] - s_hipsRef[2]) * k;
                    s_hipsRef[3] += (sgn*hq[3] - s_hipsRef[3]) * k;
                    const float n = std::sqrt(s_hipsRef[0]*s_hipsRef[0] + s_hipsRef[1]*s_hipsRef[1]
                                            + s_hipsRef[2]*s_hipsRef[2] + s_hipsRef[3]*s_hipsRef[3]);
                    if (n > 1e-4f) { s_hipsRef[0]/=n; s_hipsRef[1]/=n; s_hipsRef[2]/=n; s_hipsRef[3]/=n; }
                }
                ++s_hipsN;
            } else {
                hq[0]=s_hipsRef[0]; hq[1]=s_hipsRef[1]; hq[2]=s_hipsRef[2]; hq[3]=s_hipsRef[3];
            }
        }
    }
}

// GIRDLE TRANSLATION PIN. Locomotion/turn/weapon animations write local TRANSLATIONS
// into the shoulder-girdle chain (measured while strafing: upper-arm socket displaced
// +-5.7cm laterally, forearm segment +11%, avatar arms 0.5423 vs 0.6268 -- asymmetric!).
// Rotation-only solvers cannot fix moved sockets/lengths: the visible result was "торс
// поворачивается, руки отъезжают" on strafe and the arm double on snap-turn (turn-assist
// anims do the same for a few frames). Capture each bone's local translation over the
// first ~90 solves (idle, unarmed -- re-run via g_VRPoseCapGen on VRIK re-enable), then
// pin them every solve BEFORE the IK: geometry becomes anatomy-constant, animations can
// only rotate.
void VRIK_PinGirdleTranslations(uint8_t* boneBuf) {
    static int   s_gen = -1;
    static int   s_n = 0;
    static float s_ref[8][3];
    if (s_gen != g_VRPoseCapGen) { s_gen = g_VRPoseCapGen; s_n = 0; }
    int idx[8];
    idx[0] = (g_VRRightUpperArmIdx >= 0 && g_VRRightUpperArmIdx < VRIK_MAX_BONES)
             ? g_VRBoneParent[g_VRRightUpperArmIdx] : -1;   // right clavicle
    idx[1] = g_VRRightUpperArmIdx;
    idx[2] = g_VRRightForeArmIdx;
    idx[3] = g_VRRightBoneIdx;                              // right hand
    idx[4] = (g_VRLeftUpperArmIdx >= 0 && g_VRLeftUpperArmIdx < VRIK_MAX_BONES)
             ? g_VRBoneParent[g_VRLeftUpperArmIdx] : -1;    // left clavicle
    idx[5] = g_VRLeftUpperArmIdx;
    idx[6] = g_VRLeftForeArmIdx;
    idx[7] = g_VRLeftBoneIdx;                               // left hand
    for (int k = 0; k < 8; ++k)
        if (idx[k] < 0 || idx[k] >= VRIK_MAX_BONES) return; // chain unresolved: skip
    if (s_n < 90) {
        const float w = 1.0f / static_cast<float>(s_n + 1);
        for (int k = 0; k < 8; ++k) {
            const float* t = reinterpret_cast<const float*>(boneBuf + idx[k] * 48 + VRIK_TRANS_OFF);
            if (s_n == 0) { s_ref[k][0]=t[0]; s_ref[k][1]=t[1]; s_ref[k][2]=t[2]; }
            else {
                s_ref[k][0] += (t[0] - s_ref[k][0]) * w;
                s_ref[k][1] += (t[1] - s_ref[k][1]) * w;
                s_ref[k][2] += (t[2] - s_ref[k][2]) * w;
            }
        }
        ++s_n;
        return;
    }
    for (int k = 0; k < 8; ++k) {
        float* t = reinterpret_cast<float*>(boneBuf + idx[k] * 48 + VRIK_TRANS_OFF);
        t[0] = s_ref[k][0]; t[1] = s_ref[k][1]; t[2] = s_ref[k][2];
    }
}

// Two-bone arm IK in model space. Rotates upper arm + forearm so the wrist reaches the model-space
// target. Algorithm follows a production-grade VR full-body solver (a baseline skeleton solver,
// `setArms`), cross-validated against established arm-IK and VR-IK conventions.
//
// Approach (different from a naive "law of cosines at the shoulder"):
//   * xDir   = unit vector from HAND -> SHOULDER (so anchored at the user's actual hand position).
//   * yDir   = unit vector PERPENDICULAR to xDir, in the plane the elbow should sit; built from
//              anatomical hints (down, back, side) projected onto the plane normal to xDir.
//   * Cosine law gives the WRIST angle (the angle of the upper/forearm/hand triangle measured at
//     the hand), so elbow = handPos + xDir * cos(wrist)*foreLen + yDir * sin(wrist)*foreLen.
//   * If hand is out of reach (hsLen > upLen+foreLen), the upper+forearm lengths are stretched
//     proportionally to cover the gap. This is what allows the arm to go COMPLETELY STRAIGHT when
//     extended — clamping the distance back to armLen-eps locks in a permanent bend (the F4VR
//     code explicitly does this stretch instead, see comments below).
//
// The elbow-direction logic uses a body-frame anatomical hint that's stable everywhere:
//   * Default: elbow points DOWN (project -bodyUp onto plane perpendicular to xDir).
//   * Fallback when arm is vertical (axis nearly parallel to -bodyUp): use -bodyFwd (back).
//   * Cross-body correction: when hand reaches across midline, swing elbow OUT to the own side.
//   * Near-straight fade: as |handToShoulder| approaches armLen, fade the cross-body swing to 0
//     so the elbow doesn't twitch sideways during a fully-extended pose.
// Quaternion from an orthonormal basis built on X (primary) + Y-hint (Gram-Schmidt).
static inline void VRIK_QuatFromAxes(const float* x, const float* yHint, float* outQ) {
    float X[3] = { x[0], x[1], x[2] }; VRIK_Norm3(X);
    float Y[3] = { yHint[0], yHint[1], yHint[2] };
    float d = VRIK_Dot3(Y, X); Y[0]-=X[0]*d; Y[1]-=X[1]*d; Y[2]-=X[2]*d;
    if (VRIK_Norm3(Y) < 1e-5f) {
        Y[0]=X[1]; Y[1]=X[2]; Y[2]=X[0];
        d = VRIK_Dot3(Y, X); Y[0]-=X[0]*d; Y[1]-=X[1]*d; Y[2]-=X[2]*d; VRIK_Norm3(Y);
    }
    float Z[3]; VRIK_Cross3(X, Y, Z);
    const float m00=X[0], m01=Y[0], m02=Z[0];
    const float m10=X[1], m11=Y[1], m12=Z[1];
    const float m20=X[2], m21=Y[2], m22=Z[2];
    float tr = m00 + m11 + m22;
    if (tr > 0.0f) {
        float s = std::sqrt(tr + 1.0f) * 2.0f;
        outQ[3]=0.25f*s; outQ[0]=(m21-m12)/s; outQ[1]=(m02-m20)/s; outQ[2]=(m10-m01)/s;
    } else if (m00 > m11 && m00 > m22) {
        float s = std::sqrt(1.0f + m00 - m11 - m22) * 2.0f;
        outQ[3]=(m21-m12)/s; outQ[0]=0.25f*s; outQ[1]=(m01+m10)/s; outQ[2]=(m02+m20)/s;
    } else if (m11 > m22) {
        float s = std::sqrt(1.0f + m11 - m00 - m22) * 2.0f;
        outQ[3]=(m02-m20)/s; outQ[0]=(m01+m10)/s; outQ[1]=0.25f*s; outQ[2]=(m12+m21)/s;
    } else {
        float s = std::sqrt(1.0f + m22 - m00 - m11) * 2.0f;
        outQ[3]=(m10-m01)/s; outQ[0]=(m02+m20)/s; outQ[1]=(m12+m21)/s; outQ[2]=0.25f*s;
    }
    VRIK_QuatNorm(outQ);
}
// Model rotation R with R*aLoc = aW and R*hLoc = hW (both pairs orthonormalised on their
// primary): R = Basis(aW,hW) * Basis(aLoc,hLoc)^-1. Fully determines a bone's SWING AND TWIST
// from its anatomical axis (aLoc: bone-local segment axis, a rig CONSTANT captured at runtime)
// and its anatomical hinge (hLoc: bone-local elbow-hinge axis, also a rig constant).
static inline void VRIK_QuatAlignTwo(const float* aLoc, const float* hLoc,
                                     const float* aW, const float* hW, float* outQ) {
    float qw[4]; VRIK_QuatFromAxes(aW, hW, qw);
    float ql[4]; VRIK_QuatFromAxes(aLoc, hLoc, ql);
    float qlc[4] = { -ql[0], -ql[1], -ql[2], ql[3] };
    VRIK_QuatMul(qw, qlc, outQ); VRIK_QuatNorm(outQ);
}
// Swing/twist decomposition: the twist component of q around a unit axis.
static inline void VRIK_QuatExtractTwist(const float* q, const float* axis, float* outT) {
    float d = q[0]*axis[0] + q[1]*axis[1] + q[2]*axis[2];
    outT[0]=axis[0]*d; outT[1]=axis[1]*d; outT[2]=axis[2]*d; outT[3]=q[3];
    float n = std::sqrt(outT[0]*outT[0]+outT[1]*outT[1]+outT[2]*outT[2]+outT[3]*outT[3]);
    if (n < 1e-6f) { outT[0]=outT[1]=outT[2]=0.0f; outT[3]=1.0f; return; }
    outT[0]/=n; outT[1]/=n; outT[2]/=n; outT[3]/=n;
    if (outT[3] < 0.0f) { outT[0]=-outT[0]; outT[1]=-outT[1]; outT[2]=-outT[2]; outT[3]=-outT[3]; }
}

// Signed twist angle of childModel relative to parentModel about a LOCAL unit axis
// (axis expressed in the parent/child local frame the relative quat lives in):
// rel = conj(parentModel) * childModel (shortest form), angle = 2*atan2(dot(rel.xyz, axis), rel.w).
static inline float VRIK_TwistAngleAbout(const float* parentModel, const float* childModel,
                                         const float* axisLocal) {
    float pc[4] = { -parentModel[0], -parentModel[1], -parentModel[2], parentModel[3] };
    float rel[4]; VRIK_QuatMul(pc, childModel, rel);
    if (rel[3] < 0.0f) { rel[0]=-rel[0]; rel[1]=-rel[1]; rel[2]=-rel[2]; rel[3]=-rel[3]; }
    const float p = rel[0]*axisLocal[0] + rel[1]*axisLocal[1] + rel[2]*axisLocal[2];
    return 2.0f * std::atan2(p, rel[3]);
}

// REVERTED TO THE 0.1.18 SOLVER, on the user's call: with it the shoulders sat right, the elbows
// behaved better and the biceps did not stretch. The elbow direction is built the VRArmIK way -- a
// rest pose (armRest/bendRest) swung onto the current shoulder->hand axis, then deviated by a formula
// in body-normalised coordinates -- instead of being derived from one anatomical hint per case; the
// forward-bend strip and the outward reference are part of that construction. The min-flex clamp that
// the replacement added is gone with it.
// Upper-arm REST local translation, per side (0 = right, 1 = left). Captured on the first solve and
// used by the shoulder protraction inside VRIK_SolveArm, which writes that translation ABSOLUTE
// against it. File scope rather than a static inside the function because the WHEEL GRAB has to be
// able to put the same rest value BACK when it hands the arm to the driving animation -- the
// protraction write is not reverted by anything else (iPowerTech, 425d4262).
float g_vrikUpArmRest[2][3] = {};
bool  g_vrikUpArmRestCap[2] = { false, false };

void VRIK_SolveArm(uint8_t* boneBuf, int upperIdx, int foreIdx, int handIdx,
                                 const float* targetModel, const float* handModelRot,
                                 const float* bodyRight, const float* bodyUp, const float* bodyFwd,
                                 float poleAngleRad, float swingGain, bool isLeft,
                                 bool storeDbg) {
    if (upperIdx < 0 || foreIdx < 0 || handIdx < 0) return;
    if (upperIdx >= VRIK_MAX_BONES || foreIdx >= VRIK_MAX_BONES || handIdx >= VRIK_MAX_BONES) return;

    const float* sh = g_fkPos[upperIdx];
    const float* el = g_fkPos[foreIdx];
    const float* wr = g_fkPos[handIdx];

    float curUp[3]   = { el[0]-sh[0], el[1]-sh[1], el[2]-sh[2] };
    float curFore[3] = { wr[0]-el[0], wr[1]-el[1], wr[2]-el[2] };
    float upLen   = VRIK_Norm3(curUp);
    float foreLen = VRIK_Norm3(curFore);
    if (upLen < 1e-4f || foreLen < 1e-4f) return;

    // STABLE SEGMENT LENGTHS (anti-ratchet). The HAND PIN below writes the hand bone's local
    // translation, and the engine does NOT reset it next frame -- so reading segment lengths from
    // the live FK let the pin cap feed on its own output and ratchet the forearm out to 0.5-0.9m
    // (measured). Use deterministic lengths instead: the native rest lengths cached on the very
    // first solve of the session, or the calibrated per-segment length (userArmLen * 0.5, matching
    // VRIK_ScaleArmBonesFromRest's 50/50 split) when calibration is present.
    static float s_restUpLen[2] = { 0.0f, 0.0f }, s_restForeLen[2] = { 0.0f, 0.0f };
    const int sideJ = isLeft ? 1 : 0;
    if (s_restUpLen[sideJ] <= 0.0f) { s_restUpLen[sideJ] = upLen; s_restForeLen[sideJ] = foreLen; }
    {
        const float ual = isLeft ? g_VRUserArmLenL : g_VRUserArmLenR;
        const float calibHalf = (ual >= 0.45f) ? ual * 0.5f : 0.0f;
        upLen   = (calibHalf > 0.0f) ? calibHalf : s_restUpLen[sideJ];
        foreLen = (calibHalf > 0.0f) ? calibHalf : s_restForeLen[sideJ];
    }
    // SHOULDER PROTRACTION (VRArmIK ShoulderPoser-lite). Reaching far FORWARD a real
    // shoulder slides several cm forward (scapula protracts); the avatar's fixed shoulder
    // made forward reaches land ~5cm short, so the hand pin stretched the forearm
    // rubber-style ("как в One Piece"). Slide the upper-arm root toward the reach (mostly
    // forward, a touch out), written ABSOLUTE against the captured rest local translation
    // (the engine keeps our writes; incremental would creep).
    float shP[3] = { sh[0], sh[1], sh[2] };
    {
        const int par = g_VRBoneParent[upperIdx];
        if (par >= 0 && par < upperIdx) {
            float* tl = reinterpret_cast<float*>(boneBuf + upperIdx * 48 + VRIK_TRANS_OFF);
            if (!g_vrikUpArmRestCap[sideJ]) {
                g_vrikUpArmRest[sideJ][0]=tl[0]; g_vrikUpArmRest[sideJ][1]=tl[1]; g_vrikUpArmRest[sideJ][2]=tl[2];
                g_vrikUpArmRestCap[sideJ] = true;
            }
            // Clean base shoulder = parent FK * rest local (undo last frame's write, which
            // is already baked into g_fkPos[upperIdx]).
            float base[3]; VRIK_QuatRotateVec(g_fkRot[par], g_vrikUpArmRest[sideJ], base);
            base[0] += g_fkPos[par][0]; base[1] += g_fkPos[par][1]; base[2] += g_fkPos[par][2];
            float toT[3] = { targetModel[0]-base[0], targetModel[1]-base[1], targetModel[2]-base[2] };
            const float dist = std::sqrt(toT[0]*toT[0] + toT[1]*toT[1] + toT[2]*toT[2]);
            const float armL0 = upLen + foreLen;
            if (dist > 1e-4f && armL0 > 1e-4f) {
                float fwdness = (toT[0]*bodyFwd[0] + toT[1]*bodyFwd[1] + toT[2]*bodyFwd[2]) / dist;
                if (fwdness < 0.0f) fwdness = 0.0f;
                float need = (dist / armL0 - 0.90f) * (1.0f / 0.12f);
                if (need < 0.0f) need = 0.0f; if (need > 1.0f) need = 1.0f;
                const float pr = need * fwdness;
                const float outS = isLeft ? 1.0f : -1.0f;   // own-side outward = -sideSign*right
                float delta[3] = {
                    (bodyFwd[0]*0.06f - outS*bodyRight[0]*0.015f) * pr,
                    (bodyFwd[1]*0.06f - outS*bodyRight[1]*0.015f) * pr,
                    (bodyFwd[2]*0.06f - outS*bodyRight[2]*0.015f) * pr,
                };
                shP[0] = base[0] + delta[0]; shP[1] = base[1] + delta[1]; shP[2] = base[2] + delta[2];
                float pc[4] = { -g_fkRot[par][0], -g_fkRot[par][1], -g_fkRot[par][2], g_fkRot[par][3] };
                float ld[3]; VRIK_QuatRotateVec(pc, delta, ld);
                tl[0] = g_vrikUpArmRest[sideJ][0] + ld[0];
                tl[1] = g_vrikUpArmRest[sideJ][1] + ld[1];
                tl[2] = g_vrikUpArmRest[sideJ][2] + ld[2];
            }
        }
    }

    // Hand -> shoulder (F4VR convention; xDir is along this).
    float handToShoulder[3] = { shP[0]-targetModel[0], shP[1]-targetModel[1], shP[2]-targetModel[2] };
    float hsLen = std::sqrt(handToShoulder[0]*handToShoulder[0]
                          + handToShoulder[1]*handToShoulder[1]
                          + handToShoulder[2]*handToShoulder[2]);
    if (hsLen < 1e-4f) return; // hand on top of shoulder; no defined direction
    float xDir[3] = { handToShoulder[0]/hsLen, handToShoulder[1]/hsLen, handToShoulder[2]/hsLen };

    // STRETCH instead of clamp (per F4VR). If the user reaches farther than the rest-pose arm
    // length, we keep the arm STRAIGHT by lengthening upLen+foreLen proportionally to cover hsLen.
    // The cosine law then computes a wrist angle of 0 (cos=1, sin=0) -> elbow lies on the
    // hand-shoulder line -> arm is perfectly straight. NOT clamping with an epsilon is the fix
    // for "не могу выпрямить руку вниз" — that bug came from forcing hsLen < upLen+foreLen.
    float upL = upLen, foreL = foreLen;
    if (hsLen > upL + foreL) {
        float diff = hsLen - upL - foreL;
        float ratio = foreL / (foreL + upL);
        foreL += ratio * diff;
        upL   += (1.0f - ratio) * diff;
    }
    // Hand far closer than |up-fore|: math has no solution; equalise to keep solver stable.
    if (hsLen < std::fabs(upL - foreL) + 1e-3f) {
        float avg = (upL + foreL) * 0.5f;
        upL = foreL = avg;
    }

    // EXTENSION SNAP. With arms hanging at the sides the real shoulder->controller distance
    // comes up a few cm short of the calibrated arm length (shoulder drop/adduction, palm-
    // center grip), so the cosine law held a permanent 10-15deg micro-bend ("по швам не
    // разгибает"). Within the last reach percents, ease the segment lengths toward exactly
    // the distance -> the arm straightens fully; max shrink a few cm, visually invisible.
    {
        const float sum0 = upL + foreL;
        if (sum0 > 1e-4f && hsLen < sum0) {
            const float r = hsLen / sum0;
            float s = (r - 0.90f) * (1.0f / 0.06f);
            if (s > 1.0f) s = 1.0f;
            if (s > 0.0f) {
                const float f = 1.0f + (r - 1.0f) * s;
                upL *= f; foreL *= f;
            }
        }
    }

    // ANATOMICAL TWIST RIG CONSTANTS (captured once per side, pose-invariant; used by the
    // geometric bend below AND the twist basis further down). The bone-local segment axis
    // conj(rot)*worldAxis and the bone-local ELBOW-HINGE axis conj(rot)*worldHinge are rigid
    // rig data. Sampled from the native pose whenever the elbow is visibly bent
    // (|upDir x foreDir| > ~10deg), hinge sign = upDir x foreDir (anatomically positive).
    // s_palmLoc = hand-bone-local PALM/finger axis (forearm continuation at capture): rotating
    // the wrist swings handRot*s_palmLoc, which feeds the FinalIK-style bend vector so
    // pronation/supination pulls the elbow naturally.
    static float s_axLocUp[2][3], s_hgLocUp[2][3], s_axLocFore[2][3], s_hgLocFore[2][3];
    static float s_palmLoc[2][3];
    static float s_armRest[2][3], s_bendRest[2][3];  // native arm axis + elbow bend dir (model)
    static float s_twLoc[2][3][4], s_twAx[2][3][3];  // twist bones: base local rot + local axis
    static float s_thCap[2] = { 0.0f, 0.0f };        // hand-vs-forearm twist at capture
    static bool  s_rigCap[2] = { false, false };
    const int sideI = isLeft ? 1 : 0;
    if (!s_rigCap[sideI]) {
        float cx[3]; VRIK_Cross3(curUp, curFore, cx);
        float cl = std::sqrt(cx[0]*cx[0] + cx[1]*cx[1] + cx[2]*cx[2]);
        if (cl > 0.17f) {
            cx[0]/=cl; cx[1]/=cl; cx[2]/=cl;
            const float* ur = g_fkRot[upperIdx];
            const float* fr = g_fkRot[foreIdx];
            const float* hr = g_fkRot[handIdx];
            float uc[4] = { -ur[0], -ur[1], -ur[2], ur[3] };
            float fc[4] = { -fr[0], -fr[1], -fr[2], fr[3] };
            float hc[4] = { -hr[0], -hr[1], -hr[2], hr[3] };
            VRIK_QuatRotateVec(uc, curUp,   s_axLocUp[sideI]);
            VRIK_QuatRotateVec(uc, cx,      s_hgLocUp[sideI]);
            VRIK_QuatRotateVec(fc, curFore, s_axLocFore[sideI]);
            VRIK_QuatRotateVec(fc, cx,      s_hgLocFore[sideI]);
            VRIK_QuatRotateVec(hc, curFore, s_palmLoc[sideI]);
            // Anatomical NEUTRAL for the VRArmIK swivel model: the native animation's arm
            // axis + the direction the ELBOW POKES. Convention check (triangle S-E-W): the
            // solver's yDir is the direction the elbow is DISPLACED from the shoulder-hand
            // line; the forearm's perpendicular component points the OPPOSITE way (from the
            // elbow back across the line). So store the NEGATED perpendicular -- capturing
            // it un-negated bent every elbow backwards.
            s_armRest[sideI][0] = curUp[0]; s_armRest[sideI][1] = curUp[1]; s_armRest[sideI][2] = curUp[2];
            const float dUF = VRIK_Dot3(curFore, curUp);
            float bnd[3] = { curUp[0]*dUF - curFore[0], curUp[1]*dUF - curFore[1], curUp[2]*dUF - curFore[2] };
            if (VRIK_Norm3(bnd) > 1e-3f) {
                s_bendRest[sideI][0]=bnd[0]; s_bendRest[sideI][1]=bnd[1]; s_bendRest[sideI][2]=bnd[2];
                // Twist chain ABSOLUTE base: local rotation + bone-local forearm axis captured
                // ONCE, plus the native hand twist at capture. Per frame the twist bones are
                // rewritten as base * axisAngle(w * (solvedTwist - captureTwist)) -- writing
                // relative to the LIVE local accumulated (the engine does not re-animate these
                // helpers every frame, so a post-multiply spiralled the forearm over time).
                s_thCap[sideI] = VRIK_TwistAngleAbout(fr, hr, s_axLocFore[sideI]);
                const int* twC = isLeft ? g_VRForeTwistL : g_VRForeTwistR;
                for (int t = 0; t < 3; ++t) {
                    const int bi = twC[t];
                    if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
                    const float* bl = reinterpret_cast<const float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
                    s_twLoc[sideI][t][0]=bl[0]; s_twLoc[sideI][t][1]=bl[1];
                    s_twLoc[sideI][t][2]=bl[2]; s_twLoc[sideI][t][3]=bl[3];
                    const float* trq = g_fkRot[bi];
                    float tcq[4] = { -trq[0], -trq[1], -trq[2], trq[3] };
                    VRIK_QuatRotateVec(tcq, curFore, s_twAx[sideI][t]);
                }
                s_rigCap[sideI] = true;
            }
        }
    }

    // yDir: anatomical elbow bend direction (perpendicular to xDir = hand->shoulder axis).
    //
    // Anatomical reality: a human elbow NEVER bends forward. The bend always has a back+down
    // component relative to the body, never forward. So we BLEND down + back (NOT either-or),
    // and explicitly clamp out any forward component before normalising.
    //
    // The cross-body swing (when hand reaches across midline) blends yDir toward the own-side
    // OUT-direction; that does NOT introduce forward bend either because we re-project after.
    float sideSign = isLeft ? 1.0f : -1.0f;
    float downRef[3] = { -bodyUp[0],            -bodyUp[1],            -bodyUp[2]            };
    float backRef[3] = { -bodyFwd[0],           -bodyFwd[1],           -bodyFwd[2]           };
    float outRef[3]  = { -sideSign*bodyRight[0],-sideSign*bodyRight[1],-sideSign*bodyRight[2] };

    auto projectPerp = [](const float* v, const float* axis, float* out) {
        float d = v[0]*axis[0] + v[1]*axis[1] + v[2]*axis[2];
        out[0] = v[0] - axis[0]*d;
        out[1] = v[1] - axis[1]*d;
        out[2] = v[2] - axis[2]*d;
    };
    auto stripForwardBend = [&](float* dir) {
        float fc = VRIK_Dot3(dir, bodyFwd);
        if (fc > 0.0f) {
            dir[0] -= bodyFwd[0] * fc;
            dir[1] -= bodyFwd[1] * fc;
            dir[2] -= bodyFwd[2] * fc;
        }
        projectPerp(dir, xDir, dir);
    };
    float oPerp[3];
    projectPerp(outRef,  xDir, oPerp);
    (void)downRef; (void)backRef;

    // FinalIK-STYLE GEOMETRIC BEND (VRIK ArmSolver.GetBendNormal — the formula VRChat-class
    // products ship). Replaces the manual down+back+out blend: the bend vector is DERIVED from
    // (a) the target direction in chest space, (b) the current upper-arm axis, and (c) the hand
    // PALM axis (wrist pronation/supination pulls the elbow, FinalIK's `b -= handRot*palmAxis`).
    // Neutral rest: elbow back+down; forward reach: elbow down; high reach: elbow swings out —
    // all emergent, no per-zone constants. Chest coords (right,fwd,up), X mirrored for LEFT.
    // Every safety net below (anti-forward clamp, degenerate fallback, cross-body swing, pole
    // slider trim, straight-arm fade) still applies on top.
    float yDir[3];
    bool yFromVRArmIK = false;
    // VRArmIK SWIVEL MODEL (Parger et al., zone constants re-tuned below).
    // Zero-reference = a SYNTHETIC SYMMETRIC NEUTRAL swung onto the live target direction.
    // (v1 captured the neutral from the native animation per side -- but the two arms
    // captured in DIFFERENT animation frames, so the neutrals differed and mirrored inputs
    // produced visibly different elbows: "рассинхрон рук". The analytic neutral is mirror-
    // identical by construction.) Relaxed arm: hangs mostly down, slightly forward; elbow
    // pokes back, slightly out, slightly down. Deviation = zone model, a PURE function of
    // the normalized hand position in shoulder space; all safety clamps below still apply.
    {
        float dirN0[3] = { -xDir[0], -xDir[1], -xDir[2] };
        float armRest[3] = { -bodyUp[0] + 0.15f*bodyFwd[0] + 0.10f*outRef[0],
                             -bodyUp[1] + 0.15f*bodyFwd[1] + 0.10f*outRef[1],
                             -bodyUp[2] + 0.15f*bodyFwd[2] + 0.10f*outRef[2] };
        VRIK_Norm3(armRest);
        float bendRest[3] = { -0.90f*bodyFwd[0] + 0.35f*outRef[0] - 0.25f*bodyUp[0],
                              -0.90f*bodyFwd[1] + 0.35f*outRef[1] - 0.25f*bodyUp[1],
                              -0.90f*bodyFwd[2] + 0.35f*outRef[2] - 0.25f*bodyUp[2] };
        VRIK_Norm3(bendRest);
        float qsw[4]; VRIK_QuatFromTo(armRest, dirN0, qsw);
        float ref[3]; VRIK_QuatRotateVec(qsw, bendRest, ref);
        float refP[3]; projectPerp(ref, xDir, refP);
        if (VRIK_Norm3(refP) > 0.2f) {
            const float axLen = upLen + foreLen;
            float v[3] = { -handToShoulder[0], -handToShoulder[1], -handToShoulder[2] };
            const float nx = VRIK_Dot3(v, bodyRight) / axLen;
            const float ny = VRIK_Dot3(v, bodyUp)    / axLen;
            const float nz = VRIK_Dot3(v, bodyFwd)   / axLen;
            const float inward = nx * (isLeft ? 1.0f : -1.0f);   // + when crossing midline
            float dev = -60.0f * ny;
            float zGate = 0.6f - nz; if (zGate < 0.0f) zGate = 0.0f;
            // zWeightTop reduced 260 -> 70: at face height the stock top-zone flared the
            // elbow far outward (high shoulder strain). 70 UNDER-cancels the -60*y term
            // there -> the elbow eases slightly DOWN from neutral, only a touch out.
            dev += (ny > 0.0f) ? (70.0f * zGate * ny) : (-100.0f * zGate * (-ny));
            float xT = inward + 0.1f; if (xT < 0.0f) xT = 0.0f;
            dev += -50.0f * xT;
            if (dev >  25.0f) dev =  25.0f;   // asymmetric cap: outward/up flare is the risky side
            if (dev < -60.0f) dev = -60.0f;
            const float a = dev * 0.01745329252f * (isLeft ? 1.0f : -1.0f);
            float cxv[3]; VRIK_Cross3(xDir, refP, cxv);
            const float ca = std::cos(a), sa = std::sin(a);
            yDir[0] = refP[0]*ca + cxv[0]*sa;
            yDir[1] = refP[1]*ca + cxv[1]*sa;
            yDir[2] = refP[2]*ca + cxv[2]*sa;
            yFromVRArmIK = true;
        }
    }
    if (!yFromVRArmIK) {
        const float mir = isLeft ? -1.0f : 1.0f;
        float dirN[3] = { -xDir[0], -xDir[1], -xDir[2] };        // shoulder -> hand (unit)
        auto toChest = [&](const float* v, float* o) {
            o[0] = VRIK_Dot3(v, bodyRight) * mir;
            o[1] = VRIK_Dot3(v, bodyFwd);
            o[2] = VRIK_Dot3(v, bodyUp);
        };
        auto fromChest = [&](const float* c, float* o) {
            o[0] = bodyRight[0]*c[0]*mir + bodyFwd[0]*c[1] + bodyUp[0]*c[2];
            o[1] = bodyRight[1]*c[0]*mir + bodyFwd[1]*c[1] + bodyUp[1]*c[2];
            o[2] = bodyRight[2]*c[0]*mir + bodyFwd[2]*c[1] + bodyUp[2]*c[2];
        };
        float dC[3]; toChest(dirN, dC);
        // b = FromTo(down, dir+fwd) * back   (chest space)
        float f0[3] = { 0.0f, 0.0f, -1.0f };
        float t0[3] = { dC[0], dC[1] + 1.0f, dC[2] };
        if (VRIK_Norm3(t0) < 1e-3f) { t0[0]=dC[0]; t0[1]=dC[1]; t0[2]=dC[2]; }
        float q0[4]; VRIK_QuatFromTo(f0, t0, q0);
        float back0[3] = { 0.0f, -1.0f, 0.0f };
        float bC[3]; VRIK_QuatRotateVec(q0, back0, bC);
        float b[3]; fromChest(bC, b);
        // + target direction (NOT the live arm axis: feeding the CURRENT pose back in made
        // the bend depend on our own previous solve -> two attractors -> elbow TELEPORT).
        // b is now a PURE function of the target position: continuous, deterministic.
        b[0] += dirN[0]; b[1] += dirN[1]; b[2] += dirN[2];
        // Palm/wrist term DISABLED (user: rotating the wrist must not move the elbow).
        // Kept for future tuning: kPalmW ~0.3 would give FinalIK-style pronation coupling.
        const float kPalmW = 0.0f;
        if (kPalmW > 0.0f && s_rigCap[sideI]) {
            float pw[3]; VRIK_QuatRotateVec(handModelRot, s_palmLoc[sideI], pw);
            b[0] -= pw[0]*kPalmW; b[1] -= pw[1]*kPalmW; b[2] -= pw[2]*kPalmW;
        }
        // Elbow direction = b rejected onto the plane perpendicular to the arm axis
        // (cross(dir, cross(b, dir)) collapses to exactly this).
        projectPerp(b, xDir, yDir);
    }

    // CRITICAL anti-forward-bend clamp: project out any +bodyFwd component. The elbow MUST
    // never point in the forward direction; if the blend ended up with a forward component
    // (e.g. axis is mostly down and dPerp picked up forward), strip it here.
    stripForwardBend(yDir);

    if (VRIK_Norm3(yDir) < 0.3f) {
        // The blend degenerated (axis parallel to both bodyUp and bodyFwd, which is impossible
        // for a real body). Fall back to own-side outward.
        if (VRIK_Norm3(oPerp) > 0.3f) {
            yDir[0]=oPerp[0]; yDir[1]=oPerp[1]; yDir[2]=oPerp[2];
        } else {
            float fb[3] = { 0.0f, 0.0f, 1.0f };
            if (std::fabs(xDir[2]) > 0.9f) { fb[0]=1.0f; fb[2]=0.0f; }
            projectPerp(fb, xDir, yDir);
            VRIK_Norm3(yDir);
        }
    }

    // Cross-body swing: if the hand reaches across the body's midline, blend yDir toward the
    // own-side outward direction so the elbow follows out (anatomically correct). Fade off as
    // the arm approaches full extension (reach01 -> 1) — at straight extension yDir doesn't
    // matter, but if you keep blending you'll see a sideways elbow twitch right at the limit.
    float armLen = upL + foreL;
    float reach01 = (armLen > 1e-4f) ? (hsLen / armLen) : 0.0f;
    if (reach01 > 1.0f) reach01 = 1.0f;
    float straightFade = (reach01 - 0.92f) * (1.0f / 0.08f);
    if (straightFade < 0.0f) straightFade = 0.0f;
    if (straightFade > 1.0f) straightFade = 1.0f;
    float poleWeight = 1.0f - straightFade;

    float shoulderToHand[3] = { -handToShoulder[0], -handToShoulder[1], -handToShoulder[2] };
    float lateral = (shoulderToHand[0]*bodyRight[0]
                   + shoulderToHand[1]*bodyRight[1]
                   + shoulderToHand[2]*bodyRight[2]) * sideSign;
    float crossAmount = (lateral > 0.0f) ? (lateral / armLen) : 0.0f;
    if (VRIK_Norm3(oPerp) > 1e-3f && poleWeight > 0.0f) {
        float bendFactor = (0.90f - reach01) * (1.0f / 0.35f);
        if (bendFactor < 0.0f) bendFactor = 0.0f; if (bendFactor > 1.0f) bendFactor = 1.0f;
        float crossGate = (crossAmount - 0.05f) * (1.0f / 0.10f);
        if (crossGate < 0.0f) crossGate = 0.0f; if (crossGate > 1.0f) crossGate = 1.0f;
        // HEIGHT GATE: the cross-body swing is a CHEST-level aid. Raised to face level it
        // flared the elbow far out (strained pose, humans keep the elbow down there) --
        // fade the swing off as the hand rises above the shoulder line.
        float upAmt = (shoulderToHand[0]*bodyUp[0] + shoulderToHand[1]*bodyUp[1]
                     + shoulderToHand[2]*bodyUp[2]) / armLen;
        float hGate = 1.0f - (upAmt - 0.05f) * (1.0f / 0.30f);
        if (hGate < 0.0f) hGate = 0.0f; if (hGate > 1.0f) hGate = 1.0f;
        float f = swingGain * crossGate * bendFactor * poleWeight * hGate;
        if (f < 0.0f) f = 0.0f; if (f > 1.0f) f = 1.0f;
        yDir[0] = (1.0f-f)*yDir[0] + f*oPerp[0];
        yDir[1] = (1.0f-f)*yDir[1] + f*oPerp[1];
        yDir[2] = (1.0f-f)*yDir[2] + f*oPerp[2];
    }
    // Final anti-forward-bend clamp after blending. Elbow must never point forward.
    stripForwardBend(yDir);
    VRIK_Norm3(yDir);

    // Optional swivel (Elbow pole slider): rotate yDir around xDir by poleAngleRad. Faded by
    // poleWeight so the spin can't yank the elbow near full extension.
    {
        float swivelRad = poleAngleRad * poleWeight;
        if (std::fabs(swivelRad) > 1e-5f) {
            float c = std::cos(swivelRad), s = std::sin(swivelRad);
            float cr[3]; VRIK_Cross3(xDir, yDir, cr);
            float ad = VRIK_Dot3(xDir, yDir);
            float y2[3] = {
                yDir[0]*c + cr[0]*s + xDir[0]*ad*(1.0f-c),
                yDir[1]*c + cr[1]*s + xDir[1]*ad*(1.0f-c),
                yDir[2]*c + cr[2]*s + xDir[2]*ad*(1.0f-c),
            };
            yDir[0]=y2[0]; yDir[1]=y2[1]; yDir[2]=y2[2];
            VRIK_Norm3(yDir);
        }
    }
    stripForwardBend(yDir);
    if (VRIK_Norm3(yDir) < 1e-3f) {
        projectPerp(backRef, xDir, yDir);
        if (VRIK_Norm3(yDir) < 1e-3f) {
            projectPerp(outRef, xDir, yDir);
            VRIK_Norm3(yDir);
        }
    }

    // NEAR-EXTENSION RELAX: as the arm approaches full stretch, ease the residual bend
    // direction to anatomical DOWN(+slightly back). A nearly-straight arm hangs its tiny
    // elbow sag downward (T-pose!); holding the zone-model direction there read as
    // "не разгибается" -- the elbow poked sideways/up at 97% extension.
    {
        float rf = (reach01 - 0.88f) * (1.0f / 0.10f);
        if (rf > 1.0f) rf = 1.0f;
        if (rf > 0.0f) {
            float dn[3] = { -bodyUp[0] - 0.3f*bodyFwd[0],
                            -bodyUp[1] - 0.3f*bodyFwd[1],
                            -bodyUp[2] - 0.3f*bodyFwd[2] };
            float dnP[3]; projectPerp(dn, xDir, dnP);
            if (VRIK_Norm3(dnP) > 1e-3f) {
                yDir[0] = yDir[0]*(1.0f-rf) + dnP[0]*rf;
                yDir[1] = yDir[1]*(1.0f-rf) + dnP[1]*rf;
                yDir[2] = yDir[2]*(1.0f-rf) + dnP[2]*rf;
                if (VRIK_Norm3(yDir) < 1e-3f) { yDir[0]=dnP[0]; yDir[1]=dnP[1]; yDir[2]=dnP[2]; }
            }
        }
    }

    // TEMPORAL SMOOTHING (anti-teleport). The bend direction is now a pure function of the
    // target, but crossing workspace zones (chest reach, overhead) can still swing it fast --
    // the elbow visibly TELEPORTED between poses. A real arm re-poses smoothly: exponentially
    // smooth yDir over time (tau = 80ms) and cap the swing rate at 540 deg/s, then re-project
    // perpendicular to the arm axis (the cosine-law placement assumes yDir ⊥ xDir).
    {
        static float s_ySm[2][3];
        static bool  s_yInit[2] = { false, false };
        static long long s_yT[2] = { 0, 0 };
        LARGE_INTEGER qn, qf;
        QueryPerformanceCounter(&qn); QueryPerformanceFrequency(&qf);
        float dt = 0.016f;
        if (s_yT[sideI] != 0) {
            dt = static_cast<float>(static_cast<double>(qn.QuadPart - s_yT[sideI]) / static_cast<double>(qf.QuadPart));
            if (dt < 0.0f) dt = 0.0f;
            if (dt > 0.05f) dt = 0.05f;
        }
        s_yT[sideI] = qn.QuadPart;
        if (!s_yInit[sideI]) {
            s_ySm[sideI][0]=yDir[0]; s_ySm[sideI][1]=yDir[1]; s_ySm[sideI][2]=yDir[2];
            s_yInit[sideI] = true;
        } else {
            float alpha = 1.0f - std::exp(-dt / 0.08f);
            float dotp = s_ySm[sideI][0]*yDir[0] + s_ySm[sideI][1]*yDir[1] + s_ySm[sideI][2]*yDir[2];
            if (dotp < -1.0f) dotp = -1.0f;
            if (dotp >  1.0f) dotp =  1.0f;
            const float ang = std::acos(dotp);
            const float maxStep = 9.42477f * dt;              // 540 deg/s
            if (ang > 1e-4f && ang * alpha > maxStep) alpha = maxStep / ang;
            float ns[3] = {
                s_ySm[sideI][0] + (yDir[0]-s_ySm[sideI][0])*alpha,
                s_ySm[sideI][1] + (yDir[1]-s_ySm[sideI][1])*alpha,
                s_ySm[sideI][2] + (yDir[2]-s_ySm[sideI][2])*alpha,
            };
            if (VRIK_Norm3(ns) > 1e-3f) {
                s_ySm[sideI][0]=ns[0]; s_ySm[sideI][1]=ns[1]; s_ySm[sideI][2]=ns[2];
            } else {
                // Exactly antiparallel lerp degenerated: snap (next frames smooth from here).
                s_ySm[sideI][0]=yDir[0]; s_ySm[sideI][1]=yDir[1]; s_ySm[sideI][2]=yDir[2];
            }
        }
        float yP[3]; projectPerp(s_ySm[sideI], xDir, yP);
        if (VRIK_Norm3(yP) > 1e-3f) { yDir[0]=yP[0]; yDir[1]=yP[1]; yDir[2]=yP[2]; }
    }

    // ANATOMICAL SEPARATION (hard, applied LAST). Two facts in one clamp: (a) the humerus
    // cannot internally rotate the elbow across the body's midline at all, and (b) a human
    // elbow practically never rests flat ON the torso -- the arm always keeps a few degrees
    // of abduction ("локоть всегда слегка под углом от груди"). Enforce a MINIMUM OUTWARD
    // component (~7 deg) on the bend direction; more outward / back / down remain free.
    {
        float oP[3]; projectPerp(outRef, xDir, oP);
        if (VRIK_Norm3(oP) > 1e-3f) {
            const float minOut = 0.12f;                // ~sin(7 deg)
            const float dOut = VRIK_Dot3(yDir, oP);
            if (dOut < minOut) {
                yDir[0] += oP[0] * (minOut - dOut);
                yDir[1] += oP[1] * (minOut - dOut);
                yDir[2] += oP[2] * (minOut - dOut);
                projectPerp(yDir, xDir, yDir);
                if (VRIK_Norm3(yDir) < 1e-3f) {
                    yDir[0]=oP[0]; yDir[1]=oP[1]; yDir[2]=oP[2];
                }
            }
        }
    }

    // F4VR formula: wristAngle = acos((fore^2 + hs^2 - up^2) / (2*fore*hs)). This is the angle of
    // the triangle measured AT THE HAND, not the shoulder. Elbow position is then offset from the
    // hand by (cos*fore) along xDir + (sin*fore) along yDir.
    float cosWrist = (foreL*foreL + hsLen*hsLen - upL*upL) / (2.0f * foreL * hsLen);
    if (cosWrist < -1.0f) cosWrist = -1.0f; if (cosWrist > 1.0f) cosWrist = 1.0f;
    float sinWrist = std::sqrt(std::fmax(0.0f, 1.0f - cosWrist*cosWrist));

    float xDist = cosWrist * foreL;
    float yDist = sinWrist * foreL;

    // ---- ELBOW POLICY: close to the body, never above the wrist or the shoulder ----------------
    //
    // Two rules, both taken from how VR arm solvers actually behave rather than from what a free IK
    // picks, and both matching what a relaxed arm does.
    //
    // (1) NEAR-SHOULDER REST. VRArmIK blends the elbow toward a FIXED local direction once the hand
    //     comes within half an arm of the shoulder (useFixedElbowWhenNearShoulder: startBelowDistance
    //     0.5, weight 2, localElbowPos (0.3, -1, -2) -- a little outward, down, and well back). That
    //     is the pose a relaxed arm takes with the hand near the chest, and it is what stops the elbow
    //     from flaring out or hunting when the triangle is nearly degenerate. Their weight is a spring
    //     gain; here it is a direct blend, so it is scaled to 0.75 at the shoulder.
    //
    // (2) HEIGHT CAP. A relaxed elbow hangs BELOW the wrist and BELOW the shoulder. It only comes up
    //     when the hand itself is raised above shoulder height -- held out level, the elbow is level
    //     anyway, which the cap allows because the wrist is then the binding limit.
    //
    //     Enforced by SWIVELLING the bend direction about the arm axis, never by moving the elbow:
    //     the elbow lives on a circle around the shoulder-hand line and the segment lengths depend on
    //     it staying there. The swivel is solved in closed form. Writing yDir = a*u + b*v with
    //     u = normalize(bodyUp perpendicular to xDir) and v = xDir x u, the elbow's height is
    //     linear in a, so the cap is just a <= aMax; clamp a and rebuild b with its sign kept, which
    //     keeps the elbow on the same (outward) side it was already on.
    if (CyberpunkVR_VrikElbowPolicy && yDist > 1e-4f) {
        const float armLenTot = upL + foreL;
        if (armLenTot > 1e-3f) {
            float w = 1.0f - hsLen / (0.5f * armLenTot);
            if (w < 0.0f) w = 0.0f;
            if (w > 1.0f) w = 1.0f;
            w *= 0.75f;
            if (w > 0.0f) {
                const float rest[3] = {
                    0.30f * outRef[0] - bodyUp[0] - 2.0f * bodyFwd[0],
                    0.30f * outRef[1] - bodyUp[1] - 2.0f * bodyFwd[1],
                    0.30f * outRef[2] - bodyUp[2] - 2.0f * bodyFwd[2] };
                float rp[3]; projectPerp(rest, xDir, rp);
                if (VRIK_Norm3(rp) > 1e-3f) {
                    yDir[0] = yDir[0]*(1.0f-w) + rp[0]*w;
                    yDir[1] = yDir[1]*(1.0f-w) + rp[1]*w;
                    yDir[2] = yDir[2]*(1.0f-w) + rp[2]*w;
                    projectPerp(yDir, xDir, yDir);
                    if (VRIK_Norm3(yDir) < 1e-3f) { yDir[0]=rp[0]; yDir[1]=rp[1]; yDir[2]=rp[2]; }
                }
            }
        }
        const float handUp = VRIK_Dot3(targetModel, bodyUp);
        const float shUp   = VRIK_Dot3(shP, bodyUp);
        if (handUp <= shUp + 0.05f) {                       // arm not raised above the shoulder
            const float capUp = (handUp < shUp) ? handUp : shUp;
            float u[3]; projectPerp(bodyUp, xDir, u);
            const float r = VRIK_Norm3(u);                  // normalises u, returns the old length
            if (r > 1e-3f) {
                const float baseUp = handUp + xDist * VRIK_Dot3(xDir, bodyUp);
                float aMax = (capUp - baseUp) / (yDist * r);
                if (aMax < -1.0f) aMax = -1.0f;
                if (aMax <  1.0f) {                         // >= 1 cannot be violated
                    float v[3]; VRIK_Cross3(xDir, u, v); VRIK_Norm3(v);
                    const float a = VRIK_Dot3(yDir, u);
                    if (a > aMax) {
                        const float b = VRIK_Dot3(yDir, v);
                        float bNew = std::sqrt(std::fmax(0.0f, 1.0f - aMax * aMax));
                        if (b < 0.0f) bNew = -bNew;
                        yDir[0] = u[0]*aMax + v[0]*bNew;
                        yDir[1] = u[1]*aMax + v[1]*bNew;
                        yDir[2] = u[2]*aMax + v[2]*bNew;
                        VRIK_Norm3(yDir);
                    }
                }
            }
        }
    }

    float newElbow[3] = {
        targetModel[0] + xDir[0]*xDist + yDir[0]*yDist,
        targetModel[1] + xDir[1]*xDist + yDir[1]*yDist,
        targetModel[2] + xDir[2]*xDist + yDir[2]*yDist,
    };

    float desUp[3] = { newElbow[0]-shP[0], newElbow[1]-shP[1], newElbow[2]-shP[2] };
    VRIK_Norm3(desUp);
    float desFore[3] = { targetModel[0]-newElbow[0], targetModel[1]-newElbow[1], targetModel[2]-newElbow[2] };
    VRIK_Norm3(desFore);

    // (Rig constants captured above, before the geometric bend — see s_rigCap block.)

    // Desired hinge in model space: perpendicular to the solved bend plane (spanned by desUp and
    // desFore), anatomical sign = desUp x desFore. Near full extension the cross degenerates ->
    // fall back to the pole plane (hinge = yDir x desUp... i.e. cross(desUp, -yDir)): the forearm
    // flexes toward -yDir from the shoulder-hand line, matching the capture convention.
    float hDes[3]; VRIK_Cross3(desUp, desFore, hDes);
    {
        float hl = std::sqrt(hDes[0]*hDes[0] + hDes[1]*hDes[1] + hDes[2]*hDes[2]);
        if (hl < 0.05f) {
            float negY[3] = { -yDir[0], -yDir[1], -yDir[2] };
            VRIK_Cross3(desUp, negY, hDes);
            VRIK_Norm3(hDes);
        } else { hDes[0]/=hl; hDes[1]/=hl; hDes[2]/=hl; }
    }

    // Upper arm: base = plain swing (shortest arc from the NATIVE pose -> keeps the animation's
    // natural bicep roll). The anatomical basis is used only as a LIMIT REFERENCE: extract how
    // far the basis solution would twist the segment vs the plain swing, and apply that roll
    // only in EXTREMES -- 25deg dead zone, then eased, capped at 60deg (user: "бицепс только в
    // крайнем случае должен подворачиваться").
    float delta1[4]; VRIK_QuatFromTo(curUp, desUp, delta1);
    float swingUp[4]; VRIK_QuatMul(delta1, g_fkRot[upperIdx], swingUp); VRIK_QuatNorm(swingUp);
    float newUpModel[4] = { swingUp[0], swingUp[1], swingUp[2], swingUp[3] };
    if (s_rigCap[sideI]) {
        float basis[4]; VRIK_QuatAlignTwo(s_axLocUp[sideI], s_hgLocUp[sideI], desUp, hDes, basis);
        float swc[4] = { -swingUp[0], -swingUp[1], -swingUp[2], swingUp[3] };
        float d[4]; VRIK_QuatMul(basis, swc, d); VRIK_QuatNorm(d);
        float tw[4]; VRIK_QuatExtractTwist(d, desUp, tw);
        float proj = tw[0]*desUp[0] + tw[1]*desUp[1] + tw[2]*desUp[2];
        float angS = 2.0f * std::atan2(proj, tw[3]);           // signed twist vs plain swing
        const float kDead = 0.4363f;                           // 25 deg dead zone
        const float kCap  = 1.0472f;                           // 60 deg max applied roll
        float mag = std::fabs(angS);
        float eff = (mag <= kDead) ? 0.0f : std::fmin(mag - kDead, kCap);
        if (eff > 0.0f) {
            float s = (angS >= 0.0f ? 1.0f : -1.0f) * std::sin(eff * 0.5f);
            float aa[4] = { desUp[0]*s, desUp[1]*s, desUp[2]*s, std::cos(eff * 0.5f) };
            VRIK_QuatMul(aa, swingUp, newUpModel); VRIK_QuatNorm(newUpModel);
        }
    }

    int upParent = g_VRBoneParent[upperIdx];
    const float* upParentModel = (upParent >= 0 && upParent < VRIK_MAX_BONES) ? g_fkRot[upParent] : nullptr;
    float identity[4] = { 0,0,0,1 };
    VRIK_WriteLocalRot(boneBuf, upperIdx, upParentModel ? upParentModel : identity, newUpModel);

    // Forearm: hinge-consistent basis + LIGHT pronation: the forearm should only SLIGHTLY
    // follow the wrist roll (user feedback: 0.7 was far too much) -- 25% of the hand's twist
    // about the forearm axis, capped at 45deg; the rest stays in the wrist/hand.
    float newForeModel[4];
    if (s_rigCap[sideI]) {
        float fr0[4]; VRIK_QuatAlignTwo(s_axLocFore[sideI], s_hgLocFore[sideI], desFore, hDes, fr0);
        float frc[4] = { -fr0[0], -fr0[1], -fr0[2], fr0[3] };
        float rel[4]; VRIK_QuatMul(handModelRot, frc, rel); VRIK_QuatNorm(rel);
        float tw[4];  VRIK_QuatExtractTwist(rel, desFore, tw);
        float proj2 = tw[0]*desFore[0] + tw[1]*desFore[1] + tw[2]*desFore[2];
        float angS2 = 2.0f * std::atan2(proj2, tw[3]);
        float eff2 = angS2 * 0.25f;                            // light follow
        const float kProCap = 0.7854f;                         // 45 deg
        if (eff2 >  kProCap) eff2 =  kProCap;
        if (eff2 < -kProCap) eff2 = -kProCap;
        float s2 = std::sin(eff2 * 0.5f);
        float aa2[4] = { desFore[0]*s2, desFore[1]*s2, desFore[2]*s2, std::cos(eff2 * 0.5f) };
        VRIK_QuatMul(aa2, fr0, newForeModel); VRIK_QuatNorm(newForeModel);
    } else {
        float foreBase[3]; VRIK_QuatRotateVec(delta1, curFore, foreBase);
        float delta2[4]; VRIK_QuatFromTo(foreBase, desFore, delta2);
        float tmp[4]; VRIK_QuatMul(delta2, delta1, tmp);
        VRIK_QuatMul(tmp, g_fkRot[foreIdx], newForeModel); VRIK_QuatNorm(newForeModel);
    }
    VRIK_WriteLocalRot(boneBuf, foreIdx, newUpModel, newForeModel);

    // DISTRIBUTED STRETCH (elbow half of the hand pin). Any residual length mismatch used
    // to materialize ONLY in the hand pin -> 100% of the stretch showed on the forearm
    // (rubber-arm look). Write the ELBOW bone's parent-local translation too, so the upper
    // arm carries its share; caps keep it within anatomical looks.
    float elbowW[3] = { newElbow[0], newElbow[1], newElbow[2] };
    {
        float relU[3] = { newElbow[0]-shP[0], newElbow[1]-shP[1], newElbow[2]-shP[2] };
        const float rlU = std::sqrt(relU[0]*relU[0] + relU[1]*relU[1] + relU[2]*relU[2]);
        if (rlU > 1e-4f) {
            float clU = rlU;
            const float loU = upLen * 0.85f, hiU = upLen * 1.15f;
            if (clU < loU) clU = loU; if (clU > hiU) clU = hiU;
            elbowW[0] = shP[0] + relU[0]*(clU/rlU);
            elbowW[1] = shP[1] + relU[1]*(clU/rlU);
            elbowW[2] = shP[2] + relU[2]*(clU/rlU);
            VRIK_WriteLocalPos(boneBuf, foreIdx, shP, newUpModel, elbowW);
        }
    }

    // Hand orientation written local to the new forearm.
    VRIK_WriteLocalRot(boneBuf, handIdx, newForeModel, handModelRot);

    // FOREARM TWIST DISTRIBUTION (VRArmIK rotateHand, extended to this rig's 3-bone chain).
    // Route the DELTA of the wrist's twist about the forearm axis (our solved hand vs the
    // native animation's hand) into r/l_forearmTwist01..03 with growing weights toward the
    // wrist. Pronation/supination then skins the forearm gradually like a real radius/ulna
    // instead of snapping 100% at the wrist joint -- and the elbow stays put (the swivel
    // model above deliberately ignores wrist rotation).
    if (s_rigCap[sideI]) {
        const int* tw = isLeft ? g_VRForeTwistL : g_VRForeTwistR;
        // ABSOLUTE formulation: local = capturedBase * axisAngle(w * (solvedTwist - captureTwist)).
        // The previous incremental version post-multiplied the LIVE local rotation each solve;
        // the engine does not re-animate these helper bones every frame, so the increments
        // ACCUMULATED and the forearm skin wound up ("предплечье сильно вращается").
        const float th1 = VRIK_TwistAngleAbout(newForeModel, handModelRot, s_axLocFore[sideI]);
        float dth = th1 - s_thCap[sideI];
        while (dth >  3.14159265f) dth -= 6.28318531f;
        while (dth < -3.14159265f) dth += 6.28318531f;
        if (dth >  2.0944f) dth =  2.0944f;                       // sanity cap +-120 deg
        if (dth < -2.0944f) dth = -2.0944f;
        static const float twW[3] = { 0.2f, 0.4f, 0.6f };         // elbow -> wrist (softened)
        for (int t = 0; t < 3; ++t) {
            const int bi = tw[t];
            if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
            const float half = 0.5f * dth * twW[t];
            const float sh2 = std::sin(half), ch2 = std::cos(half);
            const float* axB = s_twAx[sideI][t];
            float rq[4] = { axB[0]*sh2, axB[1]*sh2, axB[2]*sh2, ch2 };
            float* bl = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
            float nl[4]; VRIK_QuatMul(s_twLoc[sideI][t], rq, nl); VRIK_QuatNorm(nl);
            bl[0]=nl[0]; bl[1]=nl[1]; bl[2]=nl[2]; bl[3]=nl[3];
        }
    }

    // HAND PIN (hand == target EXACTLY, user principle "кисть = gizmo, остальное подстраивается").
    // The two-bone solve lands short whenever the avatar arm and the real reach disagree; instead
    // of letting the wrist float off the gizmo, write the hand bone's parent-local TRANSLATION so
    // the wrist sits ON the target. The visual forearm segment stretches/shrinks within anatomical
    // caps (0.7..1.3 of the native segment) -- the F4VR trade: exact hand presence over a few cm
    // of forearm skin stretch.
    {
        // Anchored at the WRITTEN elbow (elbowW), not the ideal one -- the hand's parent
        // model position is wherever the fore bone actually went.
        float rel[3] = { targetModel[0]-elbowW[0], targetModel[1]-elbowW[1], targetModel[2]-elbowW[2] };
        float rl = std::sqrt(rel[0]*rel[0] + rel[1]*rel[1] + rel[2]*rel[2]);
        if (rl > 1e-4f) {
            float cl = rl;
            const float lo = foreLen * 0.7f, hi = foreLen * 1.15f;   // foreLen is rest/calibrated (stable)
            if (cl < lo) cl = lo; if (cl > hi) cl = hi;
            float pin[3] = { elbowW[0]+rel[0]*(cl/rl), elbowW[1]+rel[1]*(cl/rl), elbowW[2]+rel[2]*(cl/rl) };
            VRIK_WriteLocalPos(boneBuf, handIdx, elbowW, newForeModel, pin);
        }
    }

    if (storeDbg) {
        volatile float* L = isLeft ? g_VRIKDbgLocalL : g_VRIKDbgLocal;
        L[0]=lateral; L[1]=crossAmount; L[2]=reach01; L[3]=0.0f;
        volatile float* T = isLeft ? g_VRIKDbgTargetL   : g_VRIKDbgTarget;
        volatile float* S = isLeft ? g_VRIKDbgShoulderL : g_VRIKDbgShoulder;
        volatile float* E = isLeft ? g_VRIKDbgElbowL    : g_VRIKDbgElbow;
        volatile float* N = isLeft ? g_VRIKDbgLensL     : g_VRIKDbgLens;
        T[0]=targetModel[0]; T[1]=targetModel[1]; T[2]=targetModel[2];
        S[0]=shP[0]; S[1]=shP[1]; S[2]=shP[2];
        E[0]=newElbow[0]; E[1]=newElbow[1]; E[2]=newElbow[2];
        N[0]=upL; N[1]=foreL;
    }
}

// Builds the model-space hand target + orientation from the VR controller.
//
// The controller pose in shared memory is HMD-local, exactly like the visible gizmo hands.
// hmdRel rotates it back into the recenter/body frame so the hand target stays in place when
// the user turns their head. The shoulder pivot comes from auto-calibration in the same body
// frame (OpenXR axes: X right, Y up, Z back) instead of from the animated head bone; deriving
// it from the head bone made the wrist target inherit small head-animation arcs.
//
// Math:
//   controller_body         = hmdRel * vrPos
//   hand_from_shoulder_body = controller_body - calibratedShoulderBody
//   mapLocal (game)         = (hfs.x, -hfs.z, hfs.y) * scale
//   outTarget               = shoulderModel + mapLocal + off
// HAND STOP: a hand position resolved by PHYSICS, pushed in from script, in MODEL space.
//
// Why it has to come back through the IK target at all: the drawn hand's position is produced here, by
// VRIK_SolveArm from `target`. Physics cannot move a drawn bone. So a hand that stops at a wall means a
// target that stops at the wall -- whatever holds the collider, the stop returns through this one value.
//
// The collider itself is one of the player's own capsules flipped solver-owned at runtime
// (entPhysicalBodyInterface::SetIsKinematic) and driven by bounded velocity, exactly the way the basketball
// is carried. Authoring it Dynamic in the asset was measured and rejected: a solver-owned collider component
// is not held by its bone binding, and both hand capsules free-fell to Z = -8100.
//
// Index 0 is LEFT, 1 is RIGHT.
volatile float g_VRHandStopModel[2][3] = { {0.0f,0.0f,0.0f}, {0.0f,0.0f,0.0f} };
volatile int   g_VRHandStopValid[2]    = { 0, 0 };
// A driven body always trails its target a little, and echoing that lag into the arm would read as input
// latency rather than as contact. Only a miss LARGER than this counts as being blocked, and only the part
// beyond it is applied -- so unobstructed motion is untouched and a real wall moves the hand by exactly how
// far the body could not go.
volatile float g_VRHandStopDeadband = 0.02f;

// WRIST ROTATION LOCK. While on, the hand's MODEL-space rotation is replaced with this quat right before the arm
// solve -- the same slot the controller's twist normally rides in -- so a glued hand (the reload grip) keeps the
// recorded orientation no matter how the controller rotates. Raw tracking (g_VRHandRawRot below) is published
// BEFORE the override, so scripts keep seeing pure tracking.
volatile float g_VRHandRotLock[2][4] = { {0.0f,0.0f,0.0f,1.0f}, {0.0f,0.0f,0.0f,1.0f} };
volatile int   g_VRHandRotLockOn[2]  = { 0, 0 };

// The target BEFORE any stop is applied, i.e. where tracking alone would put the hand. The driving loop must
// steer the body towards THIS, never towards the clamped result: every palm readout a script can reach is
// published after the clamp, so driving the body at that would close a loop on itself -- the hand would stay
// wherever it stopped even after the wall was gone.
volatile float g_VRHandRawModel[2][3] = { {0.0f,0.0f,0.0f}, {0.0f,0.0f,0.0f} };
volatile int   g_VRHandRawValid[2]    = { 0, 0 };
// The TRACKED hand rotation, published for the same reason: a collision shape oriented by the solved bone
// depends on the previous frame's clamp, which is a feedback loop that shakes the hand in a cluster of capsules.
volatile float g_VRHandRawRot[2][4]   = { {0.0f,0.0f,0.0f,1.0f}, {0.0f,0.0f,0.0f,1.0f} };

void VRIK_ApplyHandStop(int side, float* target, float* handRot) {
    if (side < 0 || side > 1 || !target) return;
    // published unconditionally, so the loop has a target even while no hold is active
    g_VRHandRawModel[side][0] = target[0];
    g_VRHandRawModel[side][1] = target[1];
    g_VRHandRawModel[side][2] = target[2];
    if (handRot) {
        for (int k = 0; k < 4; ++k) g_VRHandRawRot[side][k] = handRot[k];
        // the reload grip's wrist lock: the solve gets the recorded orientation instead of the controller's
        if (g_VRHandRotLockOn[side]) {
            handRot[0] = g_VRHandRotLock[side][0];
            handRot[1] = g_VRHandRotLock[side][1];
            handRot[2] = g_VRHandRotLock[side][2];
            handRot[3] = g_VRHandRotLock[side][3];
        }
    }
    g_VRHandRawValid[side] = 1;
    if (!g_VRHandStopValid[side]) return;
    const float d[3] = { g_VRHandStopModel[side][0] - target[0],
                         g_VRHandStopModel[side][1] - target[1],
                         g_VRHandStopModel[side][2] - target[2] };
    const float len = std::sqrt(d[0]*d[0] + d[1]*d[1] + d[2]*d[2]);
    const float dead = g_VRHandStopDeadband;
    if (len <= dead || len > 1.5f) return;   // 1.5 m: the body is lost, not blocked -- ignore it
    const float k = (len - dead) / len;
    target[0] += d[0] * k;
    target[1] += d[1] * k;
    target[2] += d[2] * k;
}

void VRIK_BuildHandTarget(const float* shoulderModelPos,
                                        const float* calibratedShoulderBody,
                                        const float* hmdRel,
                                        const float* vrPos, const float* vrQuat,
                                        const float* wristCorr,
                                        float scale,
                                        const float* off,
                                        float* outTarget, float* outHandRot) {
    const float s = scale;

    // Normalise hmdRel (producer composes it without renormalising; a non-unit quat would scale
    // the rotated vector by |q|^2 -> hand drifts when head moves).
    float hq[4] = { hmdRel ? hmdRel[0] : 0.0f,
                    hmdRel ? hmdRel[1] : 0.0f,
                    hmdRel ? hmdRel[2] : 0.0f,
                    hmdRel ? hmdRel[3] : 1.0f };
    if ((hq[0]*hq[0] + hq[1]*hq[1] + hq[2]*hq[2] + hq[3]*hq[3]) > 1e-4f) VRIK_QuatNorm(hq);
    else { hq[0]=0; hq[1]=0; hq[2]=0; hq[3]=1; }

    // 1. Controller in body-frame OpenXR (cancel head orientation).
    float controllerBody[3];
    VRIK_QuatRotateVec(hq, vrPos, controllerBody);

    // 2. Shoulder offset from HMD/body origin, calibrated from the same gizmo/controller poses.
    const float shoulderBody[3] = {
        calibratedShoulderBody ? calibratedShoulderBody[0] : 0.0f,
        calibratedShoulderBody ? calibratedShoulderBody[1] : 0.0f,
        calibratedShoulderBody ? calibratedShoulderBody[2] : 0.0f
    };

    // 3. Hand position relative to shoulder, body-frame OpenXR.
    float handFromShoulderBody[3] = {
        controllerBody[0] - shoulderBody[0],
        controllerBody[1] - shoulderBody[1],
        controllerBody[2] - shoulderBody[2]
    };

    // 4. Axis-swap OpenXR -> game body, then scale by user's arm-length ratio.
    float mapLocal[3] = {
         handFromShoulderBody[0] * s,
        -handFromShoulderBody[2] * s,
         handFromShoulderBody[1] * s
    };

    // 5. Anchor at the shoulder model position.
    outTarget[0] = shoulderModelPos[0] + mapLocal[0] + off[0];
    outTarget[1] = shoulderModelPos[1] + mapLocal[1] + off[1];
    outTarget[2] = shoulderModelPos[2] + mapLocal[2] + off[2];

    // Hand orientation: cancel head rotation, axis-swap, apply per-hand wrist correction.
    float baseQuat[4];
    VRIK_QuatMul(hq, vrQuat, baseQuat);
    VRIK_QuatNorm(baseQuat);
    float mapQuat[4] = { baseQuat[0], -baseQuat[2], baseQuat[1], baseQuat[3] };
    VRIK_QuatMul(mapQuat, wristCorr, outHandRot);
    VRIK_QuatNorm(outHandRot);
}

// IK-CORRECT hand target: the controller's position in MODEL space, head-independent (via
// hmdRel), anchored at the avatar's EYE (head bone horizontal, HMD eye height). Unlike the old
// VRIK_BuildHandTarget it does NOT subtract a calibrated shoulder offset or apply a reach scale
// -- the hand goes EXACTLY where the controller is relative to the head, and the arm IK just
// pivots the (length-calibrated) arm from the shoulder to it. This matches the body-IK convention (hand target =
// controller position) and the baked view (which sits on the head), so the hand lands on the
// real controller instead of an offset-shoulder approximation.
static inline void VRIK_HandTargetModelSpace(const float* eyeAnchor, const float* hmdRel,
                                       const float* vrPos, const float* vrQuat,
                                       const float* wristCorr, const float* off,
                                       float* outTarget, float* outHandRot) {
    float hq[4] = { hmdRel ? hmdRel[0] : 0.0f, hmdRel ? hmdRel[1] : 0.0f,
                    hmdRel ? hmdRel[2] : 0.0f, hmdRel ? hmdRel[3] : 1.0f };
    if ((hq[0]*hq[0]+hq[1]*hq[1]+hq[2]*hq[2]+hq[3]*hq[3]) > 1e-4f) VRIK_QuatNorm(hq);
    else { hq[0]=0; hq[1]=0; hq[2]=0; hq[3]=1; }

    float ctrlBase[3]; VRIK_QuatRotateVec(hq, vrPos, ctrlBase);   // controller in body/base frame
    float mapLocal[3] = { ctrlBase[0], -ctrlBase[2], ctrlBase[1] }; // OpenXR -> game axes
    outTarget[0] = eyeAnchor[0] + mapLocal[0] + (off ? off[0] : 0.0f);
    outTarget[1] = eyeAnchor[1] + mapLocal[1] + (off ? off[1] : 0.0f);
    outTarget[2] = eyeAnchor[2] + mapLocal[2] + (off ? off[2] : 0.0f);

    float baseQuat[4]; VRIK_QuatMul(hq, vrQuat, baseQuat); VRIK_QuatNorm(baseQuat);
    float mapQuat[4] = { baseQuat[0], -baseQuat[2], baseQuat[1], baseQuat[3] };
    VRIK_QuatMul(mapQuat, wristCorr, outHandRot);
    VRIK_QuatNorm(outHandRot);
}

// ---------------------------------------------------------------------------
// GIZMO-EXACT (1:1) full-body anchoring helpers (Phase 1 of the full-body IK rework).
//
// The visible gizmo hands are drawn (init.lua getHandWorldPose) as
//   worldPos = camPos + camQuat * mapLocalPos(rawPos),   mapLocalPos = (x,-z,y)
// i.e. PURELY camera(HMD)-relative. The old IK built its target a different way
// (hmdRel + an animated-head-derived shoulder + a reach scale), so the hand never
// matched the gizmo and the arm anchor drifted with weapon/recoil animation.
//
// These helpers reproduce the gizmo formula in the bone buffer's MODEL space, so
// the hand lands EXACTLY on the gizmo and the shoulder is anchored to the stable
// HMD frame (not the animated spine). That single change fixes: hand!=gizmo, head
// drifting on weapon draw, and the shoulder kick on fire.

// FPP camera (HMD) pose in model space, from the world camera + entity transforms pushed by
// Lua. The bone buffer's model->world transform is the entity transform (entityPos, entityQuat),
// so world->model rotation = conjugate(entityQuat). Using the FULL entity quaternion (not a
// Rz(-yaw) guess) is what makes this exact and convention-proof -- the hand then renders exactly
// on the gizmo.
bool VRIK_ComputeCamModel(float* outPos, float* outRot, float* outEntityQuat,
                          float* outPairedRot) {
    // PHASE-COHERENT MODEL TRANSFORM.
    //
    // The animation solve runs before LocateCamera for the frame it is building.  Consequently
    // g_lastLocate*(N-1) paired with CyberpunkVR_PlayerEntity*(N) was never a same-frame pair:
    //   * translation left camera(N-1)-entity(N), i.e. one whole frame of locomotion, in outPos;
    //   * rotation left the head bone one camera sample behind the view.
    // Forward motion mostly hid the positional error behind the eye; backwards motion put the old
    // avatar in front of it, and turns/running made the changing phase visible as judder.
    //
    // Position is a relative transform, so it does not need a current world position.  Consume the
    // camera/entity pair from ONE SetVRPlayerYaw push under its seqlock and convert it with the
    // entity quaternion from THAT push.  Pure world locomotion and yaw cancel before the solve sees
    // them, even if the push itself is a tick old.
    //
    // Orientation does need the current head.  AcquireFrameHeadSample is the latch PatchCamera uses
    // later in this same aim epoch, so composing from it here makes the head bone and rendered view
    // use the identical XR sample.  The fallback remains the coherent pushed relative orientation.
    if (CyberpunkVR_VrikNativeFramePair || CyberpunkVR_VrikTransformsFromPlugin) {
        VrikTransformSnapshot snap{};
        bool haveSnapshot = false;
        // MOUNTED, THE NATIVE PAIR IS THE WRONG FRAME -- and by this function's own stated principle.
        //
        // Its entity orientation is built in BodyYawFollowTick as a pure Rz(yaw) reconstructed from the
        // body-yaw store site. On foot that is exact: the player entity is upright, so yaw is all of it.
        // Seated in a car the body pitches and rolls with the shell, so a yaw-only frame is wrong by a
        // quantity that changes every frame the car moves -- which is the jitter. That same store site is
        // already known not to describe a mounted player: the [vehyaw] census caught it flat at 0.0 deg
        // while the car swung through 260, which is why the camera writer gates ViewYawFromEngine on
        // !g_isInVehicle.
        //
        // The Lua pair carries the FULL entity world quaternion (g_VREntityQ*, "world->model =
        // conjugate(this)") paired coherently with its own camera under one seqlock, so it is preferred
        // while mounted and only while mounted. Not a filter: the wobble is not noise, it is a frame
        // built from the wrong quantity.
        const bool preferFullEntityQuat = g_isInVehicle &&
                                          CyberpunkVR_VrikVehicleFullEntityQuat &&
                                          CyberpunkVR_VrikTransformsFromPlugin;
        if (preferFullEntityQuat) {
            haveSnapshot = VRIK_ReadTransformSnapshot(&snap);
            if (haveSnapshot) {
                std::atomic_ref<uint64_t>(CyberpunkVR_DebugVrikLuaPairFallback)
                    .fetch_add(1u, std::memory_order_relaxed);
            }
        }
        if (!haveSnapshot && CyberpunkVR_VrikNativeFramePair) {
            haveSnapshot = VRIK_ReadNativeTransformSnapshot(&snap);
            if (haveSnapshot) {
                std::atomic_ref<uint64_t>(CyberpunkVR_DebugVrikNativePairUsed)
                    .fetch_add(1u, std::memory_order_relaxed);
            }
        }
        if (!haveSnapshot && CyberpunkVR_VrikTransformsFromPlugin) {
            haveSnapshot = VRIK_ReadTransformSnapshot(&snap);
            if (haveSnapshot) {
                std::atomic_ref<uint64_t>(CyberpunkVR_DebugVrikLuaPairFallback)
                    .fetch_add(1u, std::memory_order_relaxed);
            }
        }
        if (haveSnapshot) {
            // An explicit invalidation is different from a transient seqlock miss: the native saw
            // a detached/cinematic camera, so feeding either its >3 m raw span or the last FPP pair
            // into the avatar would teleport the solve into a view it does not own.
            if (!snap.valid) return false;
            float pushedEntQ[4] = { snap.entityQuat[0], snap.entityQuat[1],
                                    snap.entityQuat[2], snap.entityQuat[3] };
            VRIK_QuatNorm(pushedEntQ);
            if (outEntityQuat) {
                for (int i = 0; i < 4; ++i) outEntityQuat[i] = pushedEntQ[i];
            }
            float invPushedEnt[4]; VRIK_QuatConj(pushedEntQ, invPushedEnt);
            VRIK_QuatRotateVec(invPushedEnt, snap.cameraMinusEntity, outPos);

            float pushedCamQ[4] = { snap.camQuat[0], snap.camQuat[1],
                                    snap.camQuat[2], snap.camQuat[3] };
            VRIK_QuatNorm(pushedCamQ);
            VRIK_QuatMul(invPushedEnt, pushedCamQ, outRot);
            VRIK_QuatNorm(outRot);
            if (outPairedRot) {
                for (int i = 0; i < 4; ++i) outPairedRot[i] = outRot[i];
            }

            // Mirror PatchCamera's direct-composition gates.  In a menu/shot-frame it deliberately
            // leaves the engine orientation alone; using a fresh HMD here anyway would rotate the
            // skeleton head under a view that did not rotate.  Non-default heading modes fall back
            // to the coherent pushed relative orientation because animation has no pre-write camera
            // quaternion from which PatchCamera derives those modes.
            const bool cameraUsesDirectEngineYaw =
                CyberpunkVR_CamWriteInPatch && CyberpunkVR_CamComposeAtWrite && g_headingValid &&
                CyberpunkVR_HeadingFromPreWrite && CyberpunkVR_ViewYawFromEngine &&
                CyberpunkVR_EngineBodyYawValid && !g_isInVehicle;
            if (cameraUsesDirectEngineYaw) {
                OpenXRHeadPose head{};
                if (OpenXRManager::Get().AcquireFrameHeadSample(&head) && head.valid) {
                    float wz = CyberpunkVR_EngineBodyYawZ;
                    float ww = CyberpunkVR_EngineBodyYawW;
                    if (ww < 0.0f) { wz = -wz; ww = -ww; }
                    if (wz != 0.0f || ww != 0.0f) {
                        // The body quaternion and the view heading below come from this same census
                        // pair.  Do not re-read the four separately-published PlayerEntityQuat floats.
                        float currentEntQ[4] = { 0.0f, 0.0f, wz, ww };
                        VRIK_QuatNorm(currentEntQ);
                        float invCurrentEnt[4]; VRIK_QuatConj(currentEntQ, invCurrentEnt);

                        const float bridge = BodyYawBridgeRealignForEpoch(head.frameAimEpoch);
                        float viewYaw = 2.0f * std::atan2(wz, ww) - bridge;
                        const float yawQ[4] = { 0.0f, 0.0f,
                                                std::sin(viewYaw * 0.5f),
                                                std::cos(viewYaw * 0.5f) };
                        const float pitchQ[4] = { g_headingPitchS, 0.0f, 0.0f, g_headingPitchC };
                        const float headQ[4] = { head.oriX, -head.oriZ, head.oriY, head.oriW };
                        float headingPitch[4]; VRIK_QuatMul(yawQ, pitchQ, headingPitch);
                        float worldCamQ[4]; VRIK_QuatMul(headingPitch, headQ, worldCamQ);
                        VRIK_QuatNorm(worldCamQ);
                        VRIK_QuatMul(invCurrentEnt, worldCamQ, outRot);
                        VRIK_QuatNorm(outRot);
                    }
                }
            }
            return true;
        }
    }
    if (!g_VRCamPosValid) return false;
    float entQ[4] = { g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR };
    if ((entQ[0]*entQ[0]+entQ[1]*entQ[1]+entQ[2]*entQ[2]+entQ[3]*entQ[3]) < 1e-6f) {
        entQ[0]=0; entQ[1]=0; entQ[2]=0; entQ[3]=1;
    }
    VRIK_QuatNorm(entQ);
    if (outEntityQuat) {
        for (int i = 0; i < 4; ++i) outEntityQuat[i] = entQ[i];
    }
    float invEnt[4]; VRIK_QuatConj(entQ, invEnt);   // world->model rotation
    float camQ[4] = { g_VRCamI, g_VRCamJ, g_VRCamK, g_VRCamR };
    VRIK_QuatMul(invEnt, camQ, outRot); VRIK_QuatNorm(outRot);
    if (outPairedRot) {
        for (int i = 0; i < 4; ++i) outPairedRot[i] = outRot[i];
    }
    // WELDED BODY ANCHOR (single-filter architecture). g_VRCamPairLocal* is THE one
    // stabilized (cam - entity) offset computed in SetVRTransforms; the rendered view
    // applies the IDENTICAL value (shared [124..127] -> dxgi stabilizer). Same number
    // on both sides => body and view cannot move relative to each other, and bob/kick
    // are filtered out of both simultaneously.
    float d[3];
    if (g_VRCamPairValid) {
        d[0] = g_VRCamPairLocalX;
        d[1] = g_VRCamPairLocalY;
        d[2] = g_VRCamPairLocalZ;
    } else {
        d[0] = g_VRCamPosX - g_VREntityPosX;
        d[1] = g_VRCamPosY - g_VREntityPosY;
        d[2] = g_VRCamPosZ - g_VREntityPosZ;
    }
    VRIK_QuatRotateVec(invEnt, d, outPos);
    return true;
}

void VRIK_BodyAxesFromCamYaw(const float* camModelRot,
                                           float* bodyRight, float* bodyUp, float* bodyFwd) {
    bodyUp[0] = 0.0f; bodyUp[1] = 0.0f; bodyUp[2] = 1.0f;

    float fwdLocal[3] = { 0.0f, 1.0f, 0.0f }; // OpenXR -Z forward maps to game/model +Y.
    VRIK_QuatRotateVec(camModelRot, fwdLocal, bodyFwd);
    bodyFwd[2] = 0.0f;
    if (VRIK_Norm3(bodyFwd) < 1e-4f) { bodyFwd[0]=0.0f; bodyFwd[1]=1.0f; bodyFwd[2]=0.0f; }

    VRIK_Cross3(bodyFwd, bodyUp, bodyRight);
    if (VRIK_Norm3(bodyRight) < 1e-4f) { bodyRight[0]=1.0f; bodyRight[1]=0.0f; bodyRight[2]=0.0f; }
    VRIK_Cross3(bodyUp, bodyRight, bodyFwd);
    if (VRIK_Norm3(bodyFwd) < 1e-4f) { bodyFwd[0]=0.0f; bodyFwd[1]=1.0f; bodyFwd[2]=0.0f; }
}

// Gizmo-exact hand target + HMD-anchored shoulder, both in model space.
//   target   = camModelPos + camModelRot * mapLocalPos(controllerOpenXR)   (== gizmo)
//   shoulder = camModelPos + camModelRot * mapLocalPos(shoulderOpenXR)      (HMD-anchored)
//   handRot  = camModelRot * mapLocalQuat(controllerQuat) * wristCorr
// shoulderOpenXR is the calibrated HMD-local shoulder offset (OpenXR axes: X right,
// Y up, Z back) -> mapped (x,-z,y) the SAME way as the controller, so the whole arm
// frame is consistent with the gizmo.
static inline void VRIK_BuildHandTargetGizmo(const float* camModelPos, const float* camModelRot,
                                             const float* vrPos, const float* vrQuat,
                                             const float* shoulderOpenXR, const float* wristCorr,
                                             const float* off,
                                             float* outTarget, float* outHandRot,
                                             float* outShoulderModel) {
    float mapPos[3] = { vrPos[0], -vrPos[2], vrPos[1] };
    float rotP[3];  VRIK_QuatRotateVec(camModelRot, mapPos, rotP);
    outTarget[0] = camModelPos[0] + rotP[0] + (off ? off[0] : 0.0f);
    outTarget[1] = camModelPos[1] + rotP[1] + (off ? off[1] : 0.0f);
    outTarget[2] = camModelPos[2] + rotP[2] + (off ? off[2] : 0.0f);

    float mapSh[3] = { shoulderOpenXR[0], -shoulderOpenXR[2], shoulderOpenXR[1] };
    float rotS[3];  VRIK_QuatRotateVec(camModelRot, mapSh, rotS);
    outShoulderModel[0] = camModelPos[0] + rotS[0];
    outShoulderModel[1] = camModelPos[1] + rotS[1];
    outShoulderModel[2] = camModelPos[2] + rotS[2];

    float localQuat[4] = { vrQuat[0], -vrQuat[2], vrQuat[1], vrQuat[3] };
    float hm[4]; VRIK_QuatMul(camModelRot, localQuat, hm);
    VRIK_QuatMul(hm, wristCorr, outHandRot);
    VRIK_QuatNorm(outHandRot);
}

// Arm-length calibration. A bone's parent-local translation IS its segment length, so the
// avatar arm length = |foreArm.translation| (upper-arm) + |hand.translation| (forearm).
// We scale those two translations so the avatar arm length matches the user's T-pose-measured
// arm length: a straight real arm then yields a straight avatar arm at the 1:1 gizmo target
// (fixes "arm down but elbow still bent"). Hand + fingers keep their normal size.
static inline float VRIK_ArmScale(uint8_t* boneBuf, int foreIdx, int handIdx, float userArmLen) {
    // Reject implausible T-pose measurements (real shoulder->wrist is ~0.45..0.85 m). A bad
    // calibration frame must NOT shrink the avatar arm -> fall back to the avatar's own length.
    if (userArmLen < 0.45f || userArmLen > 0.85f || foreIdx < 0 || handIdx < 0
        || foreIdx >= VRIK_MAX_BONES || handIdx >= VRIK_MAX_BONES) return 1.0f;
    auto segLen = [&](int idx) -> float {
        const float* t = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_TRANS_OFF);
        return std::sqrt(t[0]*t[0] + t[1]*t[1] + t[2]*t[2]);
    };
    float avatar = segLen(foreIdx) + segLen(handIdx);
    if (avatar < 0.05f) return 1.0f;
    float s = userArmLen / avatar;
    if (s < 0.7f) s = 0.7f; if (s > 1.3f) s = 1.3f;
    return s;
}

static inline void VRIK_ScaleArmBones(uint8_t* boneBuf, int foreIdx, int handIdx, float scale) {
    if (scale <= 0.0f || std::fabs(scale - 1.0f) < 1e-3f) return;
    if (foreIdx >= 0 && foreIdx < VRIK_MAX_BONES) {
        float* t = reinterpret_cast<float*>(boneBuf + foreIdx * 48 + VRIK_TRANS_OFF);
        t[0]*=scale; t[1]*=scale; t[2]*=scale;
    }
    if (handIdx >= 0 && handIdx < VRIK_MAX_BONES) {
        float* t = reinterpret_cast<float*>(boneBuf + handIdx * 48 + VRIK_TRANS_OFF);
        t[0]*=scale; t[1]*=scale; t[2]*=scale;
    }
}

static inline bool VRIK_ArmRestTrans(uint8_t* boneBuf, uintptr_t trackBuf, int boneCount, int idx, float* out) {
    static uintptr_t s_trackBuf = 0;
    static int s_boneCount = 0;
    static bool s_valid[VRIK_MAX_BONES] = {};
    static float s_trans[VRIK_MAX_BONES][3] = {};

    if (!boneBuf || idx < 0 || idx >= VRIK_MAX_BONES || idx >= boneCount) return false;
    if (trackBuf != s_trackBuf || boneCount != s_boneCount) {
        for (int i = 0; i < VRIK_MAX_BONES; ++i) s_valid[i] = false;
        s_trackBuf = trackBuf;
        s_boneCount = boneCount;
    }
    if (!s_valid[idx]) {
        const float* t = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_TRANS_OFF);
        s_trans[idx][0] = t[0]; s_trans[idx][1] = t[1]; s_trans[idx][2] = t[2];
        s_valid[idx] = true;
    }
    out[0] = s_trans[idx][0]; out[1] = s_trans[idx][1]; out[2] = s_trans[idx][2];
    return true;
}

void VRIK_ScaleArmBonesFromRest(uint8_t* boneBuf, uintptr_t trackBuf, int boneCount,
                                              int foreIdx, int handIdx, float userArmLen) {
    if (userArmLen < 0.45f || userArmLen > 0.85f) return;
    float foreRest[3], handRest[3];
    if (!VRIK_ArmRestTrans(boneBuf, trackBuf, boneCount, foreIdx, foreRest)) return;
    if (!VRIK_ArmRestTrans(boneBuf, trackBuf, boneCount, handIdx, handRest)) return;

    float upperLen = std::sqrt(foreRest[0]*foreRest[0] + foreRest[1]*foreRest[1] + foreRest[2]*foreRest[2]);
    float foreLen  = std::sqrt(handRest[0]*handRest[0] + handRest[1]*handRest[1] + handRest[2]*handRest[2]);
    if (upperLen < 0.02f || foreLen < 0.02f) return;

    // Human shoulder->wrist split. The old uniform scale preserved the avatar's bad bicep/forearm
    // ratio; this gives each segment its own calibrated length while preserving its rest direction.
    // Even 50/50 split. The user's real bicep/forearm are near-equal (~33/~35cm); 0.515 forearm
    // read as "forearm too long" in game (amplified by the reach stretch), and the old 0.52 bicep
    // read as bicep too long. Neutral 50/50 avoids both; revisit only if a clear asymmetry shows.
    float targetUpper = userArmLen * 0.50f;
    float targetFore  = userArmLen * 0.50f;
    float upperScale = targetUpper / upperLen;
    float foreScale  = targetFore  / foreLen;
    if (upperScale < 0.65f) upperScale = 0.65f; if (upperScale > 1.45f) upperScale = 1.45f;
    if (foreScale  < 0.65f) foreScale  = 0.65f; if (foreScale  > 1.45f) foreScale  = 1.45f;

    float* foreT = reinterpret_cast<float*>(boneBuf + foreIdx * 48 + VRIK_TRANS_OFF);
    float* handT = reinterpret_cast<float*>(boneBuf + handIdx * 48 + VRIK_TRANS_OFF);
    foreT[0] = foreRest[0] * upperScale; foreT[1] = foreRest[1] * upperScale; foreT[2] = foreRest[2] * upperScale;
    handT[0] = handRest[0] * foreScale;  handT[1] = handRest[1] * foreScale;  handT[2] = handRest[2] * foreScale;
}

// PUT THE ARM'S TRANSLATIONS BACK, for an arm handed to the driving animation (iPowerTech,
// 425d4262). Two of our writes are translations the engine does NOT revert: the segment lengths
// scaled to the player's real arm, and the shoulder protraction. Left in place, the animation's own
// rotations then put the hand BESIDE the wheel instead of on it -- so an arm we stop solving has to
// get the rig's own rest values back. Lives here because VRIK_ArmRestTrans above and the protraction
// rest pair are both this file's.
void VRIK_RestoreArmRestTrans(uint8_t* boneBuf, uintptr_t trackBuf, int boneCount,
                                            int upperIdx, int foreIdx, int handIdx, bool isLeft) {
    float rest[3];
    if (foreIdx >= 0 && foreIdx < VRIK_MAX_BONES
        && VRIK_ArmRestTrans(boneBuf, trackBuf, boneCount, foreIdx, rest)) {
        float* t = reinterpret_cast<float*>(boneBuf + foreIdx * 48 + VRIK_TRANS_OFF);
        t[0] = rest[0]; t[1] = rest[1]; t[2] = rest[2];
    }
    if (handIdx >= 0 && handIdx < VRIK_MAX_BONES
        && VRIK_ArmRestTrans(boneBuf, trackBuf, boneCount, handIdx, rest)) {
        float* t = reinterpret_cast<float*>(boneBuf + handIdx * 48 + VRIK_TRANS_OFF);
        t[0] = rest[0]; t[1] = rest[1]; t[2] = rest[2];
    }
    const int side = isLeft ? 1 : 0;
    if (upperIdx >= 0 && upperIdx < VRIK_MAX_BONES && g_vrikUpArmRestCap[side]) {
        float* t = reinterpret_cast<float*>(boneBuf + upperIdx * 48 + VRIK_TRANS_OFF);
        t[0] = g_vrikUpArmRest[side][0];
        t[1] = g_vrikUpArmRest[side][1];
        t[2] = g_vrikUpArmRest[side][2];
    }
}

// Generic 2-bone limb IK (hip->knee->foot). Rotation-only writes (no stretch). The knee bends
// toward poleDir (projected perpendicular to the hip->foot axis). Used to keep the feet planted
// on their captured ground targets after the hips move under the HMD.
static inline void VRIK_SolveLeg(uint8_t* boneBuf, int upIdx, int midIdx, int endIdx,
                                 const float* target, const float* poleDir) {
    if (upIdx < 0 || midIdx < 0 || endIdx < 0
        || upIdx >= VRIK_MAX_BONES || midIdx >= VRIK_MAX_BONES || endIdx >= VRIK_MAX_BONES) return;
    const float* hip = g_fkPos[upIdx];
    float curUp[3] = { g_fkPos[midIdx][0]-hip[0], g_fkPos[midIdx][1]-hip[1], g_fkPos[midIdx][2]-hip[2] };
    float curLo[3] = { g_fkPos[endIdx][0]-g_fkPos[midIdx][0], g_fkPos[endIdx][1]-g_fkPos[midIdx][1], g_fkPos[endIdx][2]-g_fkPos[midIdx][2] };
    float upLen = VRIK_Norm3(curUp), loLen = VRIK_Norm3(curLo);
    if (upLen < 1e-4f || loLen < 1e-4f) return;

    float toTarget[3] = { target[0]-hip[0], target[1]-hip[1], target[2]-hip[2] };
    float dist = VRIK_Norm3(toTarget);
    if (dist < 1e-4f) return;
    float maxLen = upLen + loLen;
    if (dist > maxLen * 0.999f) dist = maxLen * 0.999f;

    float pole[3] = { poleDir[0], poleDir[1], poleDir[2] };
    float pd = VRIK_Dot3(pole, toTarget);
    pole[0]-=toTarget[0]*pd; pole[1]-=toTarget[1]*pd; pole[2]-=toTarget[2]*pd;
    if (VRIK_Norm3(pole) < 1e-3f) { pole[0]=0; pole[1]=0; pole[2]=1; }

    float cosHip = (upLen*upLen + dist*dist - loLen*loLen) / (2.0f*upLen*dist);
    if (cosHip < -1.0f) cosHip = -1.0f; if (cosHip > 1.0f) cosHip = 1.0f;
    float hipAng = std::acos(cosHip);
    float kneePos[3] = {
        hip[0] + toTarget[0]*(std::cos(hipAng)*upLen) + pole[0]*(std::sin(hipAng)*upLen),
        hip[1] + toTarget[1]*(std::cos(hipAng)*upLen) + pole[1]*(std::sin(hipAng)*upLen),
        hip[2] + toTarget[2]*(std::cos(hipAng)*upLen) + pole[2]*(std::sin(hipAng)*upLen),
    };
    float desUp[3] = { kneePos[0]-hip[0], kneePos[1]-hip[1], kneePos[2]-hip[2] }; VRIK_Norm3(desUp);
    float d1[4]; VRIK_QuatFromTo(curUp, desUp, d1);
    float newUp[4]; VRIK_QuatMul(d1, g_fkRot[upIdx], newUp); VRIK_QuatNorm(newUp);
    int up_p = g_VRBoneParent[upIdx]; float id[4] = {0,0,0,1};
    VRIK_WriteLocalRot(boneBuf, upIdx, (up_p>=0 && up_p<VRIK_MAX_BONES)?g_fkRot[up_p]:id, newUp);
    float loBase[3]; VRIK_QuatRotateVec(d1, curLo, loBase);
    float desLo[3] = { target[0]-kneePos[0], target[1]-kneePos[1], target[2]-kneePos[2] }; VRIK_Norm3(desLo);
    float d2[4]; VRIK_QuatFromTo(loBase, desLo, d2);
    float tmp[4]; VRIK_QuatMul(d2, d1, tmp);
    float newLo[4]; VRIK_QuatMul(tmp, g_fkRot[midIdx], newLo); VRIK_QuatNorm(newLo);
    VRIK_WriteLocalRot(boneBuf, midIdx, newUp, newLo);
}

// PHASE 2 — FULL BODY under the HMD, anchored from the HEAD ("bone head = hmd").
// The head bone is driven to the HMD position+orientation; the spine chain bends NATURALLY and
// distributed (lower spine rounds, chest leans, neck stretches) to connect the hips up to the
// head; the hips slide under the HMD; and a 2-bone leg IK keeps the feet on their captured
// ground positions. This is the CP2077 post-eval adaptation of the standard VR body solver (setBodyUnderHMD +
// handleSpine + setLegs): we can only rewrite local transforms in the bone buffer, so we move
// the hips translation, distribute the spine rotation (CCD), IK the legs, and orient the head.
//
// Falls back to head-orient-only (no body move) when the leg bones aren't resolved, so the feet
// can never float.
void VRIK_PlaceBodyUnderHMD(uint8_t* boneBuf,
                                          const float* camModelPos, const float* camModelRot,
                                          int headIdx, const float* bodyFwd) {
    int hips = g_VRHipsIdx;
    if (hips < 0 || hips >= VRIK_MAX_BONES || headIdx < 0 || headIdx >= VRIK_MAX_BONES) return;
    float id[4] = { 0,0,0,1 };

    bool haveR = (g_VRRightFootIdx >= 0 && g_VRRightFootIdx < VRIK_MAX_BONES
                  && g_VRRightUpLegIdx >= 0 && g_VRRightUpLegIdx < VRIK_MAX_BONES
                  && g_VRRightLegIdx >= 0 && g_VRRightLegIdx < VRIK_MAX_BONES);
    bool haveL = (g_VRLeftFootIdx >= 0 && g_VRLeftFootIdx < VRIK_MAX_BONES
                  && g_VRLeftUpLegIdx >= 0 && g_VRLeftUpLegIdx < VRIK_MAX_BONES
                  && g_VRLeftLegIdx >= 0 && g_VRLeftLegIdx < VRIK_MAX_BONES);
    bool moveBody = haveR && haveL;   // only relocate the body if we can plant the feet

    // 1. Capture foot model positions to keep them planted.
    float footR[3] = {0,0,0}, footL[3] = {0,0,0};
    if (haveR) { footR[0]=g_fkPos[g_VRRightFootIdx][0]; footR[1]=g_fkPos[g_VRRightFootIdx][1]; footR[2]=g_fkPos[g_VRRightFootIdx][2]; }
    if (haveL) { footL[0]=g_fkPos[g_VRLeftFootIdx][0];  footL[1]=g_fkPos[g_VRLeftFootIdx][1];  footL[2]=g_fkPos[g_VRLeftFootIdx][2]; }

    // HEAD = CAMERA, rigidly (user's hard requirement): the head bone tracks the offset-corrected
    // game camera in XY too, so the whole upper body moves with the view as one block -> no "weapon
    // draw: camera back, body forward" desync and no "squat: head stays, body drops". After the
    // camera offset is baked/tuned, camModelPos.xy sits over the feet so the spine is vertical;
    // before tuning it leans (a cue to bake). Hips stay over the feet (legs vertical); the spine
    // bridges the small XY gap.
    float footCx = 0.5f*(footR[0]+footL[0]);
    float footCy = 0.5f*(footR[1]+footL[1]);
    // Real-life SQUAT: the game FPP camera height (camModelPos.z) is FIXED, so it can't tell when
    // the player physically crouches. shared[89] = the HMD's physical height rel the recenter base
    // (~0 standing, negative squatting); lower the whole body by that so the knees bend.
    // Squat height: use the NECK-PIVOT height (shared[90]), which removes the optical-centre arc
    // so looking DOWN no longer reads as a crouch (the #1 false-squat cause). Fall back to the raw
    // HMD height [89] if the producer hasn't written [90]. Deadzone g_VRSquatThreshold then ignores
    // small head bob; subtract it so the squat ramps from 0 smoothly.
    float squatDrop = 0.0f;
    if (g_pSharedHands) {
        float hy = SharedPose(90);
        if (hy == 0.0f) hy = SharedPose(89);
        float drop = -hy - g_VRSquatThreshold;
        if (drop > 0.0f) squatDrop = drop;
        if (squatDrop > 0.7f) squatDrop = 0.7f;
    }
    // Squat deadzone(2cm) + EMA(alpha=0.25) so sprint/jump head-bob doesn't twitch body+arms.
    // Small changes inside the deadband are frozen (ignore bob); larger real crouches ease in.
    // Shared via s_vrSharedSquatDrop so the ARM anchor uses the SAME smoothed squat as the body.
    {
        static float s_squatEMA = 0.0f; static bool s_squatInit = false;
        const float kSquatDead = 0.02f; const float kSquatA = 0.25f;
        if (!s_squatInit) { s_squatEMA = squatDrop; s_squatInit = true; }
        else if (std::fabs(squatDrop - s_squatEMA) > kSquatDead) { s_squatEMA += (squatDrop - s_squatEMA) * kSquatA; }
        squatDrop = s_squatEMA;
    }
    s_vrSharedSquatDrop = squatDrop;
    // Head anchor = camera + small head-above-eyes gap, minus the physical squat. The BODY is
    // placed naturally and is NOT dragged toward the camera mount. "Bake to eyes" is done the
    // OTHER way around (correct direction, per user): the RENDERED VIEW is moved onto the
    // avatar's eyes -- see the eye-view publish at the END of this function (slots [116..119],
    // applied view-only in dxgi LocateCamera). Body solve stays untouched by it (no feedback).
    float headAnchor[3] = { camModelPos[0], camModelPos[1], camModelPos[2] + g_VRHeadDrop - squatDrop };

    // IK-style standing base: weapon stance must not push the legs forward. Keep the current
    // foot spacing, but recenter the pair directly below the HMD/head in model XY.
    if (haveR && haveL) {
        float fcx = 0.5f * (footR[0] + footL[0]);
        float fcy = 0.5f * (footR[1] + footL[1]);
        float sx = headAnchor[0] - fcx;
        float sy = headAnchor[1] - fcy;
        footR[0] += sx; footR[1] += sy;
        footL[0] += sx; footL[1] += sy;
    }

    // 2. Lower/raise the hips to follow the HMD height (so a real squat bends the knees), keeping
    //    them over the feet. Use the VERTICAL torso height (head.z - hips.z), NOT the 3D chain
    //    distance -- otherwise a leaned-back animation pose makes the chain longer than the
    //    vertical drop and the hips sink, bending the knees even while standing straight.
    if (moveBody) {
        // [3-FRAME ENGINE-BOB FIX] torsoVert (head.z - hips.z) sets the hips height.
        // In AER the engine flexes the upper body ~9cm on a strict 3-frame cycle, so
        // this span jittered -> the hips (hence the whole body) bobbed "1 of 3 frames".
        // A 3-tap MEDIAN per the measured cycle kills the 1-in-3 outlier regardless of
        // whether the head, the hips, or both bob (median([lo,hi,hi])=hi every frame),
        // while a real crouch (a sustained change) passes after ~1 frame. torsoVert is
        // a near-constant anatomical span so filtering it has no downside.
        float torsoVertRaw = g_fkPos[headIdx][2] - g_fkPos[hips][2];
        static float s_tvHist[3] = {0,0,0};
        static int   s_tvN = 0;
        s_tvHist[s_tvN % 3] = torsoVertRaw;
        ++s_tvN;
        float torsoVert = torsoVertRaw;
        if (s_tvN >= 3) {
            const float a = s_tvHist[0], b = s_tvHist[1], c = s_tvHist[2];
            torsoVert = a < b ? (b < c ? b : (a < c ? c : a)) : (a < c ? a : (b < c ? c : b));
        }
        if (torsoVert < 0.2f) torsoVert = 0.2f;
        // Hips follow the camera XY too (rigid body block) so the whole body moves with the view as
        // one piece -- no "weapon draw: camera/head back, hips forward" torso desync. After baking
        // camModelPos.xy = foot centre, so the legs stay vertical; the leg IK keeps the feet planted.
        float hipsTarget[3] = { headAnchor[0], headAnchor[1], headAnchor[2] - torsoVert };
        int hp = g_VRBoneParent[hips];
        if (hp >= 0 && hp < VRIK_MAX_BONES) {
            VRIK_WriteLocalPos(boneBuf, hips, g_fkPos[hp], g_fkRot[hp], hipsTarget);
            VRIK_ComputeFK(boneBuf, VRIK_FKCount());
        }
    }

    // 3. Distributed spine bend (CCD). Chain = spine bones + neck. Each bone, base->tip, rotates
    //    a fraction toward putting the head over the feet at HMD height; repeated passes converge
    //    with a gradual curve (rounded lower spine, leaning chest, stretched neck).
    if (moveBody) {
        int chain[10]; int chainN = 0;
        for (int s = 0; s < g_VRSpineCount && chainN < 9; ++s)
            if (g_VRSpineIdx[s] >= 0 && g_VRSpineIdx[s] < VRIK_MAX_BONES) chain[chainN++] = g_VRSpineIdx[s];
        if (g_VRNeckIdx >= 0 && g_VRNeckIdx < VRIK_MAX_BONES && chainN < 10) chain[chainN++] = g_VRNeckIdx;

        for (int pass = 0; pass < 3; ++pass) {
            for (int c = 0; c < chainN; ++c) {
                int idx = chain[c];
                const float* pivot = g_fkPos[idx];
                // curDir MUST be the live FK head: this CCD is iterative (ComputeFK runs
                // after each bone), so the current direction has to reflect the actual
                // updated head each pass — feeding a fixed/median head here makes the
                // spine over-rotate and spasm. The upper-body bob is instead addressed
                // by stabilizing the ANCHOR target (headAnchor, from the stable camera)
                // and the hips (torsoVert median), not this baseline.
                float curDir[3] = { g_fkPos[headIdx][0]-pivot[0], g_fkPos[headIdx][1]-pivot[1], g_fkPos[headIdx][2]-pivot[2] };
                float desDir[3] = { headAnchor[0]-pivot[0], headAnchor[1]-pivot[1], headAnchor[2]-pivot[2] };
                if (VRIK_Norm3(curDir) < 1e-4f || VRIK_Norm3(desDir) < 1e-4f) continue;
                float d[4]; VRIK_QuatFromTo(curDir, desDir, d);
                float pd[4]; VRIK_QuatScale(d, 0.5f, pd);   // half the remaining error per bone
                float newModel[4]; VRIK_QuatMul(pd, g_fkRot[idx], newModel); VRIK_QuatNorm(newModel);
                int pp = g_VRBoneParent[idx];
                VRIK_WriteLocalRot(boneBuf, idx, (pp>=0&&pp<VRIK_MAX_BONES)?g_fkRot[pp]:id, newModel);
                VRIK_ComputeFK(boneBuf, VRIK_FKCount());
            }
        }
    }

    // 4. Leg IK: feet back to their captured ground positions, knees bending forward.
    if (haveR) VRIK_SolveLeg(boneBuf, g_VRRightUpLegIdx, g_VRRightLegIdx, g_VRRightFootIdx, footR, bodyFwd);
    if (haveL) VRIK_SolveLeg(boneBuf, g_VRLeftUpLegIdx,  g_VRLeftLegIdx,  g_VRLeftFootIdx,  footL, bodyFwd);
    if (moveBody) VRIK_ComputeFK(boneBuf, VRIK_FKCount());

    // 5. Head follows the real head: orient the head bone to the HMD.
    {
        int hp = g_VRBoneParent[headIdx];
        VRIK_WriteLocalRot(boneBuf, headIdx, (hp>=0&&hp<VRIK_MAX_BONES)?g_fkRot[hp]:id, camModelRot);
        VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    }

    // NOTE: the palm publish used to live here and read g_fkPos[palmBone]. That is BEFORE
    // VRIK_SolveArm runs (lines ~3300 / ~3550), so it returned the ENGINE'S ANIMATED pose -- the
    // idle animation with the hands at the thighs -- and the ball sat at the player's right hip
    // no matter where the controller was. Exactly the trap the hand-to-holster code documents a
    // few hundred lines below. The publish now happens right after each arm is solved.

    // BODY PUBLISH (VR basketball). The player's authored collision is one capsule -- radius 1.0 m
    // as shipped, and even resized it is a cylinder of uniform width. A ball cannot rest against
    // something like that the way it rests against a chest, so the ball resolves itself against
    // capsules built from these bones instead. The spine and legs are final by this point: the
    // body placement and the distributed spine bend both ran above and FK has just been recomputed.
    {
        auto pub = [&](int slot, int bone) {
            if (bone >= 0 && bone < VRIK_MAX_BONES) {
                g_VRBodyBone[slot][0] = g_fkPos[bone][0];
                g_VRBodyBone[slot][1] = g_fkPos[bone][1];
                g_VRBodyBone[slot][2] = g_fkPos[bone][2];
                g_VRBodyBoneOk[slot] = 1;
            } else {
                g_VRBodyBoneOk[slot] = 0;
            }
        };
        const int spineCount = static_cast<int>(g_VRSpineCount);
        const int chest = (spineCount > 0) ? g_VRSpineIdx[spineCount - 1] : -1;  // topmost Spine*
        const int mid   = (spineCount > 1) ? g_VRSpineIdx[spineCount / 2] : chest;
        pub(0,  g_VRHipsIdx);
        pub(1,  mid);
        pub(2,  chest);
        pub(3,  g_VRNeckIdx);
        pub(4,  g_VRHeadBoneIdx);
        pub(5,  g_VRLeftUpLegIdx);
        pub(6,  g_VRLeftLegIdx);
        pub(7,  g_VRLeftFootIdx);
        pub(8,  g_VRRightUpLegIdx);
        pub(9,  g_VRRightLegIdx);
        pub(10, g_VRRightFootIdx);
    }

    // 5b. VIEW-ANCHOR PUBLISH -- HEAD BONE + USER-TUNED CONSTANTS ("bake на head").
    // The eye-midpoint auto-measure is gone: view target = HEAD BONE + fixed offset,
    // model axes (X right, Y fwd, Z up). Values tuned by the user with the live Tracking
    // sliders AFTER the 131072 fixed-point fix (honest 1:1 meters): (-0.02, +0.10, +0.15).
    // The Tracking sliders should sit at ZERO now -- these constants replace them.
    // delta = (headFK + kViewOff) - (baked) camModelPos, published on the same [116..119]
    // channel dxgi's LocateCamera already applies view-only (next to xrHeadOffset+camBake).
    // No feedback: the view offset never feeds camModelPos or the body solve. EMA(0.1)
    // kills FK jitter; sanity clamp +-0.9m.
    if (g_pSharedHands && headIdx >= 0 && headIdx < VRIK_MAX_BONES) {
        const float kViewOffRight = -0.02f, kViewOffFwd = 0.10f, kViewOffUp = 0.15f;
        float tgt[3] = { g_fkPos[headIdx][0] + kViewOffRight,
                         g_fkPos[headIdx][1] + kViewOffFwd,
                         g_fkPos[headIdx][2] + kViewOffUp };
        float d[3] = { tgt[0]-camModelPos[0], tgt[1]-camModelPos[1], tgt[2]-camModelPos[2] };
        bool sane = true;
        for (int k = 0; k < 3; ++k) { if (!(d[k] > -0.9f && d[k] < 0.9f)) sane = false; }
        if (sane) {
            static float s_eyeViewEMA[3] = {0,0,0}; static bool s_evInit = false;
            if (!s_evInit) { s_eyeViewEMA[0]=d[0]; s_eyeViewEMA[1]=d[1]; s_eyeViewEMA[2]=d[2]; s_evInit = true; }
            else { for (int k = 0; k < 3; ++k) s_eyeViewEMA[k] += (d[k]-s_eyeViewEMA[k]) * 0.1f; }
            g_pSharedHands[116] = s_eyeViewEMA[0];
            g_pSharedHands[117] = s_eyeViewEMA[1];
            g_pSharedHands[118] = s_eyeViewEMA[2];
            g_pSharedHands[119] = 1.0f;
        }
        // NOTE: cig->mouth distance is NOT computed here. g_fkPos at this stage is still the ENGINE
        // IDLE pose (wrist ~hip); the hand only reaches the controller after the arm IK below. The
        // mouth distance is computed post-solve from the controller target -- see g_VRSmokeMouthDist.
    }

    g_VRIKDbgChest[0]=g_fkPos[headIdx][0]; g_VRIKDbgChest[1]=g_fkPos[headIdx][1]; g_VRIKDbgChest[2]=g_fkPos[headIdx][2];
    g_VRIKDbgChestTgt[0]=headAnchor[0]; g_VRIKDbgChestTgt[1]=headAnchor[1]; g_VRIKDbgChestTgt[2]=headAnchor[2];
}


// Swing-twist: extract the TWIST of q about axis a (unit, same frame as q's vector part):
// twist = normalize((v·a)a, w); identity when the projection degenerates (q ⟂ a, 180° swing).
void VRIK_TwistAbout(const float* q, const float* a, float* outT) {
    const float d = q[0]*a[0] + q[1]*a[1] + q[2]*a[2];
    outT[0] = a[0]*d; outT[1] = a[1]*d; outT[2] = a[2]*d; outT[3] = q[3];
    float n = outT[0]*outT[0] + outT[1]*outT[1] + outT[2]*outT[2] + outT[3]*outT[3];
    if (n < 1e-10f) { outT[0]=0.0f; outT[1]=0.0f; outT[2]=0.0f; outT[3]=1.0f; return; }
    n = 1.0f / std::sqrt(n);
    outT[0]*=n; outT[1]*=n; outT[2]*=n; outT[3]*=n;
}

// AnimPoseFunc_t and OriginalAnimPose moved to src/Hooks/AnimPose.cpp: the trampoline the
// detour calls through is the hook's own state, not the character solve's.

// Solve cache (one solve per tick + replay).
// The frame the cache holds a solve for, and the frame counter itself. See the note in
// Anim/CharacterRig.hpp: this used to be the Lua-pushed tick from the shared block.
uint32_t g_solveCacheTick = 0xFFFFFFFFu;
std::atomic<uint32_t> g_VrikFrameEpoch{0};

// ---- WHERE DOES THE SHAKE ENTER? ---------------------------------------------------------------
//
// The SECOND DIFFERENCE of a position, |p[n] - 2p[n-1] + p[n-2]|, in millimetres. Smooth motion --
// however fast -- has a small second difference; noise has a large one, because noise reverses
// direction every sample and that is precisely what the second difference measures. The same number
// at three stages of the hand pipeline therefore says WHERE the shake is added:
//
//   0 ctrl    the controller position as the SOLVE reads it, HMD-local (~52 Hz, irregular)
//   1 anchor  the point the arm hangs off, model space (shared by both hands)
//   2 target  the hand target the IK actually solves for, model space
//   3 xr      the SAME controller position, sampled on the XR thread (72 Hz, uniform)
//
// STAGE 3 EXISTS TO CATCH ALIASING, and it is the difference between 3 and 0 that matters. The XR
// thread publishes at 72 Hz; the solve consumes at ~52 Hz and takes whatever the newest publish was, so
// its effective sampling interval alternates between one and two XR frames. For a MOVING hand that
// alternation alone produces |v * (dt_n - dt_n-1)| of apparent noise -- at 0.15 m/s and 14 ms of
// alternation, 2.1 mm, which is the order of the 3.5 mm measured at stage 0. Stage 3 samples the same
// number uniformly, on the producer's own clock, where that term cannot arise.
//
// Stage 3 quiet and stage 0 loud => the shake is our sampling, not the tracking, and no filter belongs
// anywhere: the solve should ask for the pose at ITS OWN instant instead of consuming a faster stream.
//
// Stage 0 large  -> the tracking is noisy and nothing downstream is at fault.
// Stage 0 quiet, 1 large -> the ANCHOR shakes: the composed camera, the entity, the bob.
// Stages 0 and 1 quiet, 2 large -> we add it ourselves, in the offset composition.
//
// ALL THREE ARE SAMPLED IN THE SAME PLACE, once per fresh solve. That is not a detail: a second
// difference depends on the sampling interval, so measuring stage 0 on the XR thread at 72 Hz and the
// others in the solve at ~52 Hz would make the three numbers incomparable and the comparison is the
// entire point.
//
// A PEAK, CLEARED EACH WINDOW, not a mean: shake is an excursion, and an average over a hundred
// frames hides one bad sample in ninety-nine good ones.
//
// This instrument replaces one that never existed. Slots [224..226] were read by the rate census and
// described in its comment as exactly this measurement, and nothing in the tree ever wrote them -- so
// it printed zeros from the day it was added.
// TWO PEAKS, and the difference between them is the whole reading.
//
// A second difference assumes a UNIFORM sampling interval. Ours is not uniform -- the game produces a
// 41 ms frame every couple of seconds, measured -- and with uneven dt a perfectly smooth hand still
// scores |v * (dt_n - dt_n-1)|. At 2 m/s a 17 ms hiccup alone is 34 mm, which is the size of the peaks
// the first version of this instrument reported. It was measuring frame-time jitter and calling it
// tracking noise.
//
// So the number that answers the complaint is the SLOW one: the same second difference, accumulated
// only while the hand is moving slower than kShakeSlowMm per sample. At that speed the v*dt term
// cannot manufacture millimetres, so what is left is really noise. The ALL peak stays beside it,
// because the pair is diagnostic: all >> slow means the peaks are motion and timing, not jitter.
// How far the ENGINE had moved a bone we own, between its graph evaluation and our write, in mm.
// Peak per window. See the measurement site in src/Hooks/AnimPose.cpp: this is the size of the only
// window in which the skeleton holds somebody else's arm, and therefore the size of the error any
// renderer that samples inside it will draw.
// The batch-gap census: how many pass gaps were counted as a NEW animation batch and how many as the
// same one, with the extremes of each. The two populations must stay far apart -- passes inside a batch
// are microseconds apart, batches a frame apart -- and the moment SameMax approaches NewMin the batch
// clock has stopped being safe.
// Which pass of the animation batch found a bone of ours already moved. Index 0 is the batch's first
// pass (which takes the fresh-solve path, so it never appears here), 1..6 the later ones, 7 a catch-all.
int   g_VrikOverwritePassHist[8] = {0,0,0,0,0,0,0,0};

int   g_VrikBatchGapNew = 0;
int   g_VrikBatchGapSame = 0;
float g_VrikBatchGapNewMin = 0.0f;
float g_VrikBatchGapSameMax = 0.0f;

float g_VrikEngineOverwriteMm = 0.0f;
// Which bone it was, and how often it happened at all: a quarter-metre peak that fires in one window
// out of three is a different problem from one that fires every pass, and the bone says whose arm --
// or whose body -- is being drawn in the wrong place.
int   g_VrikEngineOverwriteBone = -1;
int   g_VrikEngineOverwriteHits = 0;      // passes where the engine had moved a bone more than 10 mm
int   g_VrikEngineOverwritePasses = 0;    // replay passes examined
float g_VrikShakePeakMm[2][4] = {{0.0f,0.0f,0.0f,0.0f},{0.0f,0.0f,0.0f,0.0f}};   // any motion
float g_VrikShakeSlowMm[2][4] = {{0.0f,0.0f,0.0f,0.0f},{0.0f,0.0f,0.0f,0.0f}};   // slow motion only
int   g_VrikShakeSlowN[2][4]  = {{0,0,0,0},{0,0,0,0}};
namespace {
struct VrikShakeHist { float p1[3]; float p2[3]; int n; };
VrikShakeHist g_vrikShakeHist[2][4] = {};
// 3 mm per sample is ~0.15 m/s at 52 fps: a deliberate, watched, slow hand -- exactly the motion the
// shake was reported at, and slow enough that timing jitter contributes microns rather than millimetres.
constexpr float kShakeSlowMm = 3.0f;
}  // namespace

void VRIK_NoteShake(int hand, int stage, const float* pos) {
    if (hand < 0 || hand > 1 || stage < 0 || stage > 3 || !pos) return;
    VrikShakeHist& h = g_vrikShakeHist[hand][stage];
    if (h.n >= 2) {
        float acc2 = 0.0f, step2 = 0.0f;
        for (int i = 0; i < 3; ++i) {
            const float d2 = pos[i] - 2.0f * h.p1[i] + h.p2[i];
            acc2 += d2 * d2;
            const float d1 = pos[i] - h.p1[i];
            step2 += d1 * d1;
        }
        const float mm = std::sqrt(acc2) * 1000.0f;
        const float stepMm = std::sqrt(step2) * 1000.0f;
        if (mm < 500.0f) {
            if (mm > g_VrikShakePeakMm[hand][stage]) g_VrikShakePeakMm[hand][stage] = mm;
            if (stepMm < kShakeSlowMm) {
                if (mm > g_VrikShakeSlowMm[hand][stage]) g_VrikShakeSlowMm[hand][stage] = mm;
                ++g_VrikShakeSlowN[hand][stage];
            }
        }
    }
    for (int i = 0; i < 3; ++i) { h.p2[i] = h.p1[i]; h.p1[i] = pos[i]; }
    if (h.n < 2) ++h.n;
}

int   g_solveCacheN = 0;
int   g_solveCacheIdx[96];
float g_solveCacheVal[96][7];
float g_solveCacheYaw = 0.0f;   // heading the cached solve was built with
float g_solveCacheSnapCtr = -1.0f; // snap event counter [147] the cached solve consumed

