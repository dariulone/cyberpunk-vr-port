// GRASP: the fingers closed on what a hand holds, from the thing's own shape and size -- not a recorded pose.
//
// A thing is a primitive -- a box, a ball, a cylinder or a cone, by its half sizes -- at a pose in the hand's weapon
// slot (WeaponRight / WeaponLeft: the slot the lobby attaches what it holds to). Its shape and size make the kind of
// grasp (Classify):
//  - WRAP: a can, a bottle, a cone, a bar (a box two of whose sizes are a can's and the third longer): across the
//    palm where the fingers start, a little diagonal (the thumb's end further out), the fingers curled round it and
//    the thumb over it;
//  - BALL: the palm under it, the fingers spread round it with their pads on it, the thumb opposite them;
//  - EDGE: a plate (a book: a box much thinner than it is wide, wider than a bar): held from below by the middle of
//    its short edge -- standing on that edge across the palm where the fingers start, its length out of the hand, the
//    four fingers up one cover and the thumb on the other;
//  - FLAT: anything bigger -- a box: the face nearest the palm onto it, held where the hand took it, so a big box
//    held in its middle barely bends the fingers and one held at its edge has them round the edge;
//  - PINCH: a small thing (no half size of 3 cm): flat on the pad of the index finger bent a little, the thumb's pad
//    on its other side.
//
// SEAT (VRGraspSeat): where in the slot the thing lies for its grasp. The palm is the hand's own, measured from the
// skeleton every pass in its reference (open) pose: the knuckles from index to pinky, their normal on the side the
// fingers close to, the slab of the palm between them and the wrist.
//
// CLOSE (VRGraspSet): the four fingers from the reference pose, spread for a ball, each joint turned about its own
// hinge -- the bone axis nearest the one that takes the segment toward the palm -- a step at a time, the knuckle
// first and each joint after it leading once the one before is done (following a little until then). A segment that comes into the thing, or (but the first) into the palm, stops its joint and every
// joint before it; the joints after it go on. A segment already in the thing where it starts is free to move out of it.
// The thumb is searched instead (SolveThumb): its base turned in front of the palm and across it, its two joints
// bent, for the pose whose pad lies on the thing where the grasp wants it, out of the thing, the palm and the closed
// fingers. Fingers are capsules (proximal 9.5 mm, distal 7.5 mm). Solved once per Set; the result is the bones'
// parent-local rotations, written every pass over what the tracking gave them, nlerped by the blend (VRGraspBlend)
// so the hand eases on and off.
//
// Everything is in the HAND bone's frame: the slot and the fingers are its children, so neither the solution nor the
// seat depends on where the arm is. While a hand holds a thing its slot is the rig's own (PinSlot): locomotion
// animates the weapon slots (a gun's sway when walking, more of it crouched), and a held thing swayed in the hand. The finger bones are the ladder profile's (VRIK_LadderFingerBone): 0 the thumb's
// metacarpal, 1-4 index..pinky metacarpals, 5 thumb proximal, 6-9 proximal phalanges, 10 thumb distal, 11-14 middle
// phalanges, 15-18 distal phalanges.
#include "Anim/GraspPose.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/VrikState.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/Transform.hpp>
#include <RED4ext/Scripting/Natives/Generated/Quaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/Vector4.hpp>
#include <algorithm>
#include <cmath>
#include <cstring>
#include <mutex>

extern "C" {
// Solves, passes that wrote a grasp, palms measured, segments that touched in the last solve.
__declspec(dllexport) volatile uint64_t CyberpunkVR_DebugGrasp[4] = {};
// Per side, the last solve: its kind (0 flat, 1 wrap, 2 ball, 3 pinch, 4 edge), the thumb's base lift and swing and its two
// joints, then index..pinky's three joints each, in degrees.
__declspec(dllexport) volatile float CyberpunkVR_DebugGraspAngles[2][17] = {};
// Per side, the palm in the slot's frame: skin centre, normal (out of the palm), along the fingers, toward the thumb.
__declspec(dllexport) volatile float CyberpunkVR_DebugGraspPalm[2][12] = {};
// Per side: 1 when the side the fingers close to and the hand's handedness disagree about the palm's normal.
__declspec(dllexport) volatile int CyberpunkVR_DebugGraspNormalFlip[2] = {};
// Per side, the thumb's search: its cost, the overlap (mm), the pad's gap to the thing (mm), the pad from its goal (mm).
__declspec(dllexport) volatile float CyberpunkVR_DebugGraspThumb[2][4] = {};
}

namespace {

enum Shape : int { kBox = 0, kSphere = 1, kCylinder = 2, kCone = 3 };
enum Kind : int { kFlat = 0, kWrap = 1, kBall = 2, kPinch = 3, kEdge = 4 };
constexpr int kSlots = 19;
constexpr float kPi = 3.14159265f;
constexpr float kDeg = kPi / 180.0f;
constexpr float kPalmSkin = 0.014f;    // the palm's skin over the plane of the hand's bones (m)
constexpr float kPinchSize = 0.03f;    // a box no half size of which reaches this is pinched (m)
constexpr float kPinchBall = 0.018f;   // a ball under this radius is pinched (m)
constexpr float kPinchPad = 0.0085f;   // the index finger's pad over its bone (m)
constexpr float kWrapThick = 0.042f;   // a box is a bar when its two smaller half sizes are within this (m)
constexpr float kWrapRadius = 0.05f;   // a cylinder or a cone up to this radius is wrapped (m)
constexpr float kWrapBack = 0.008f;     // a wrapped thing's axis behind the knuckles (m)
constexpr float kWrapTilt = 14.0f * kDeg;
constexpr float kWrapOverhang = 0.055f; // a wrapped thing's thumb-side end at least this far past the index knuckle (m)
constexpr float kFlatMargin = 0.025f;  // the palm's centre kept this far inside a held face's edge (m)
// A face or a ball lies on the palm's pads (the thenar, the hypothenar) this far over its middle's skin (m).
constexpr float kPadRise = 0.006f;
constexpr float kPlate = 0.45f;         // a box is a plate when its thinnest half size is under this share of the next
constexpr float kEdgeGap = 0.010f;      // a held plate's cover this far behind the knuckles: the fingers fold onto it (m)
constexpr float kStep = 1.5f * kDeg;
constexpr float kFollow = 0.2f;        // a finger joint's pace while the joint before it still closes

struct Pose { float p[3]{0.0f, 0.0f, 0.0f}; float q[4]{0.0f, 0.0f, 0.0f, 1.0f}; };

void Sub(const float* a, const float* b, float* o) { for (int k = 0; k < 3; ++k) o[k] = a[k] - b[k]; }
void Mad(const float* a, const float* b, float s, float* o) { for (int k = 0; k < 3; ++k) o[k] = a[k] + b[k] * s; }
float Dot(const float* a, const float* b) { return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; }
float Len(const float* a) { return std::sqrt(Dot(a, a)); }
void Cross(const float* a, const float* b, float* o) {
    o[0] = a[1] * b[2] - a[2] * b[1];
    o[1] = a[2] * b[0] - a[0] * b[2];
    o[2] = a[0] * b[1] - a[1] * b[0];
}
bool Unit(float* a) {
    const float l = Len(a);
    if (!(l > 1e-8f)) return false;
    for (int k = 0; k < 3; ++k) a[k] /= l;
    return true;
}
void Rot(const float* q, const float* v, float* o) { VRIK_QuatRotateVec(q, v, o); }
void Unrot(const float* q, const float* v, float* o) {
    float c[4];
    VRIK_QuatConj(q, c);
    VRIK_QuatRotateVec(c, v, o);
}
Pose Compose(const Pose& a, const Pose& b) {
    Pose o;
    float r[3];
    Rot(a.q, b.p, r);
    for (int k = 0; k < 3; ++k) o.p[k] = a.p[k] + r[k];
    VRIK_QuatMul(a.q, b.q, o.q);
    VRIK_QuatNorm(o.q);
    return o;
}
// A point into frame f's own coordinates.
void ToFrame(const Pose& f, const float* p, float* o) {
    float d[3];
    Sub(p, f.p, d);
    Unrot(f.q, d, o);
}
void AxisAngle(const float* axis, float rad, float* q) {
    const float s = std::sin(rad * 0.5f);
    q[0] = axis[0] * s; q[1] = axis[1] * s; q[2] = axis[2] * s; q[3] = std::cos(rad * 0.5f);
}
// The bone axis (in a frame turned by q) nearest the world axis k, when it is near enough (an anatomical hinge),
// else k itself, in q's own coordinates.
void LocalHinge(const float* q, const float* k, float* out) {
    Unrot(q, k, out);
    int best = 0;
    for (int a = 1; a < 3; ++a) if (std::fabs(out[a]) > std::fabs(out[best])) best = a;
    if (std::fabs(out[best]) > 0.85f) {
        const float s = out[best] > 0.0f ? 1.0f : -1.0f;
        out[0] = out[1] = out[2] = 0.0f;
        out[best] = s;
    }
    Unit(out);
}

Pose Local(const uint8_t* buf, int bone) {
    Pose o;
    std::memcpy(o.p, buf + bone * 48 + VRIK_TRANS_OFF, sizeof(o.p));
    std::memcpy(o.q, buf + bone * 48 + VRIK_ROT_OFF, sizeof(o.q));
    VRIK_QuatNorm(o.q);
    return o;
}

bool ValidBone(int b) { return b >= 0 && b < g_VRBoneCount && b < VRIK_MAX_BONES; }

// Bone b in the frame of its ancestor `root`, through the buffer's locals -- a finger slot's rotation replaced by
// `rot[slot]` when given. False when root is not an ancestor.
bool InFrame(const uint8_t* buf, int side, int bone, int root, const float (*rot)[4], Pose& out) {
    int chain[32];
    int n = 0;
    for (int b = bone; b != root; b = g_VRBoneParent[b]) {
        if (!ValidBone(b) || n >= 32) return false;
        chain[n++] = b;
    }
    Pose acc;
    for (int i = n - 1; i >= 0; --i) {
        Pose l = Local(buf, chain[i]);
        if (rot) {
            for (int s = 0; s < kSlots; ++s) {
                if (VRIK_LadderFingerBone(side, s) == chain[i]) { std::copy_n(rot[s], 4, l.q); break; }
            }
        }
        acc = Compose(acc, l);
    }
    out = acc;
    return true;
}

// The rig's reference rotations (the open hand), the buffer's where the reference has none.
void ReferenceFingers(const uint8_t* buf, int side, float (*start)[4], bool* have) {
    for (int s = 0; s < kSlots; ++s) {
        const int b = VRIK_LadderFingerBone(side, s);
        if (have) have[s] = false;
        if (!ValidBone(b)) { start[s][0] = start[s][1] = start[s][2] = 0.0f; start[s][3] = 1.0f; continue; }
        if (!VRIK_ReferenceFingerRotation(b, start[s])) std::copy_n(Local(buf, b).q, 4, start[s]);
        if (have) have[s] = true;
    }
}

// The fingertip in its distal bone's frame: the bone after it, or the distal bone's own reach again.
void TipOffset(const uint8_t* buf, int distal, float* tip) {
    for (int i = 0; i < g_VRBoneCount && i < VRIK_MAX_BONES; ++i) {
        if (g_VRBoneParent[i] == distal) {
            const Pose c = Local(buf, i);
            if (Len(c.p) > 0.004f) { std::copy_n(c.p, 3, tip); return; }
        }
    }
    const Pose d = Local(buf, distal);
    for (int k = 0; k < 3; ++k) tip[k] = 0.75f * d.p[k];
}

// ---------------------------------------------------------------- shapes

// The signed distance from p (in the thing's frame) to its surface. A cone is a frustum along z: radius h[0] at
// -h[2], h[1] at +h[2] (a capped cone).
float Sdf(int shape, const float* h, const float* p) {
    switch (shape) {
    case kSphere:
        return Len(p) - h[0];
    case kCylinder: {
        const float r = std::hypot(p[0], p[1]) - h[0], z = std::fabs(p[2]) - h[2];
        return std::min(std::max(r, z), 0.0f) + std::hypot(std::max(r, 0.0f), std::max(z, 0.0f));
    }
    case kCone: {
        const float q = std::hypot(p[0], p[1]), r1 = h[0], r2 = h[1], hh = h[2];
        const float k2[2] = {r2 - r1, 2.0f * hh};
        const float ca[2] = {q - std::min(q, p[2] < 0.0f ? r1 : r2), std::fabs(p[2]) - hh};
        const float t = std::clamp(((r2 - q) * k2[0] + (hh - p[2]) * k2[1]) / (k2[0] * k2[0] + k2[1] * k2[1]),
                                   0.0f, 1.0f);
        const float cb[2] = {q - r2 + k2[0] * t, p[2] - hh + k2[1] * t};
        const float s = (cb[0] < 0.0f && ca[1] < 0.0f) ? -1.0f : 1.0f;
        return s * std::sqrt(std::min(ca[0] * ca[0] + ca[1] * ca[1], cb[0] * cb[0] + cb[1] * cb[1]));
    }
    default: {
        const float q[3] = {std::fabs(p[0]) - h[0], std::fabs(p[1]) - h[1], std::fabs(p[2]) - h[2]};
        const float o[3] = {std::max(q[0], 0.0f), std::max(q[1], 0.0f), std::max(q[2], 0.0f)};
        return Len(o) + std::min(std::max(q[0], std::max(q[1], q[2])), 0.0f);
    }
    }
}

// How far the thing reaches from its centre along u (a unit vector in the slot's frame), turned by r.
float Support(int shape, const float* h, const float* r, const float* u) {
    float l[3];
    Unrot(r, u, l);
    const float side = std::sqrt(std::max(0.0f, 1.0f - l[2] * l[2]));
    switch (shape) {
    case kSphere: return h[0];
    case kCylinder: return h[0] * side + h[2] * std::fabs(l[2]);
    case kCone: return std::max(h[0], h[1]) * side + h[2] * std::fabs(l[2]);
    default: return h[0] * std::fabs(l[0]) + h[1] * std::fabs(l[1]) + h[2] * std::fabs(l[2]);
    }
}

// The grasp a thing of this shape and size takes; `axis` its long axis (a bar's, a can's: z).
Kind Classify(int shape, const float* h, int& axis) {
    axis = 2;
    if (shape == kSphere) return h[0] < kPinchBall ? kPinch : kBall;
    if (shape == kCylinder || shape == kCone) return std::max(h[0], h[1]) <= kWrapRadius ? kWrap : kFlat;
    int order[3] = {0, 1, 2};
    std::sort(order, order + 3, [&](int a, int b) { return h[a] < h[b]; });
    if (h[order[2]] < kPinchSize) return kPinch;
    axis = order[2];
    if (h[order[0]] < kPlate * h[order[1]] && h[order[1]] > kWrapThick) {
        axis = order[0];
        return kEdge;
    }
    if (h[order[1]] <= kWrapThick && h[order[2]] >= 1.5f * h[order[1]]) return kWrap;
    return kFlat;
}

// The segments' distance: between p1-q1 and p2-q2.
float SegSeg(const float* p1, const float* q1, const float* p2, const float* q2) {
    float d1[3], d2[3], r[3];
    Sub(q1, p1, d1);
    Sub(q2, p2, d2);
    Sub(p1, p2, r);
    const float a = Dot(d1, d1), e = Dot(d2, d2), f = Dot(d2, r);
    float s = 0.0f, t = 0.0f;
    if (a > 1e-10f || e > 1e-10f) {
        if (a <= 1e-10f) t = std::clamp(f / e, 0.0f, 1.0f);
        else {
            const float c = Dot(d1, r);
            if (e <= 1e-10f) s = std::clamp(-c / a, 0.0f, 1.0f);
            else {
                const float b = Dot(d1, d2), den = a * e - b * b;
                s = den > 1e-12f ? std::clamp((b * f - c * e) / den, 0.0f, 1.0f) : 0.0f;
                t = (b * s + f) / e;
                if (t < 0.0f) { t = 0.0f; s = std::clamp(-c / a, 0.0f, 1.0f); }
                else if (t > 1.0f) { t = 1.0f; s = std::clamp((b - c) / a, 0.0f, 1.0f); }
            }
        }
    }
    float c1[3], c2[3], w[3];
    Mad(p1, d1, s, c1);
    Mad(p2, d2, t, c2);
    Sub(c1, c2, w);
    return Len(w);
}

// ---------------------------------------------------------------- the palm

struct Palm {
    float knuckle[3], n[3], d[3], t[3];   // hand frame: the knuckles' middle and the palm's axes
    float base[3], halfD, halfT;           // the palm's slab: the bones' plane between the wrist and the knuckles
    float skin[3];                         // the palm's skin over its middle
    float index[3];                        // the index finger's knuckle
    float pinch[3], pinchN[3];             // the index pad bent for a pinch, and the way out of it
    Pose slot;                             // the weapon slot in the hand's frame
};

struct Finger { int slot[3]; float maxDeg[3]; float speed[3]; float radius[3]; float spread; };
const Finger kFingers[4] = {
    {{6, 11, 15}, {88.0f, 105.0f, 80.0f}, {1.0f, 1.15f, 0.85f}, {0.0095f, 0.0085f, 0.0075f}, 1.0f},
    {{7, 12, 16}, {88.0f, 105.0f, 80.0f}, {1.0f, 1.15f, 0.85f}, {0.0095f, 0.0085f, 0.0075f}, 0.0f},
    {{8, 13, 17}, {88.0f, 105.0f, 80.0f}, {1.0f, 1.15f, 0.85f}, {0.0095f, 0.0085f, 0.0075f}, -1.0f},
    {{9, 14, 18}, {88.0f, 105.0f, 80.0f}, {1.0f, 1.15f, 0.85f}, {0.009f, 0.008f, 0.007f}, -1.8f},
};
const int kThumbSlots[3] = {0, 5, 10};
const float kThumbRadius[3] = {0.012f, 0.0105f, 0.0095f};
constexpr float kPinchBend[3] = {32.0f * kDeg, 38.0f * kDeg, 16.0f * kDeg};

// A finger's chain from `rot` (its three bones' local rotations): the joints and the tip, in the hand's frame, and
// the bones' frames.
struct ChainBones {
    int bones[3];
    Pose base;
    Pose local[3];
    float tip[3];
};
bool ChainOf(const uint8_t* buf, int side, int hand, const int* slots, const float (*start)[4], ChainBones& c) {
    for (int j = 0; j < 3; ++j) {
        c.bones[j] = VRIK_LadderFingerBone(side, slots[j]);
        if (!ValidBone(c.bones[j])) return false;
    }
    if (!InFrame(buf, side, g_VRBoneParent[c.bones[0]], hand, start, c.base)) return false;
    TipOffset(buf, c.bones[2], c.tip);
    for (int j = 0; j < 3; ++j) {
        c.local[j] = Local(buf, c.bones[j]);
        std::copy_n(start[slots[j]], 4, c.local[j].q);
    }
    return true;
}
void Joints(const ChainBones& c, const float (*rot)[4], float (*J)[3], Pose* F) {
    Pose acc = c.base;
    for (int j = 0; j < 3; ++j) {
        Pose l = c.local[j];
        std::copy_n(rot[j], 4, l.q);
        acc = Compose(acc, l);
        F[j] = acc;
        std::copy_n(acc.p, 3, J[j]);
    }
    float r[3];
    Rot(F[2].q, c.tip, r);
    for (int k = 0; k < 3; ++k) J[3][k] = F[2].p[k] + r[k];
}
// Each joint's flexion hinge, in its bone's frame: the bone axis nearest seg x toward.
bool Hinges(const float (*J)[3], const Pose* F, const float* toward, float (*hinge)[3]) {
    for (int j = 0; j < 3; ++j) {
        float seg[3], k[3];
        Sub(J[j + 1], J[j], seg);
        Cross(seg, toward, k);
        if (!Unit(k)) return false;
        LocalHinge(F[j].q, k, hinge[j]);
    }
    return true;
}

// The slot in the hand's frame as the rig has it: the frame a held thing's slot is held at (PinSlot), whatever the
// animation does with it meanwhile, so a seat is the same before the hand holds and while it does.
bool SlotFrame(const uint8_t* buf, int side, int hand, int slot, const float (*ref)[4], Pose& out) {
    Pose parent;
    if (!InFrame(buf, side, g_VRBoneParent[slot], hand, ref, parent)) return false;
    Pose l = Local(buf, slot);
    float p[3], q[4];
    if (VRIK_ReferenceLocal(slot, p, q)) {
        std::copy_n(p, 3, l.p);
        std::copy_n(q, 4, l.q);
        VRIK_QuatNorm(l.q);
    }
    out = Compose(parent, l);
    return true;
}

bool MeasurePalm(const uint8_t* buf, int side, int hand, int slotBone, Palm& pm) {
    float ref[kSlots][4];
    ReferenceFingers(buf, side, ref, nullptr);
    const int index1 = VRIK_LadderFingerBone(side, 6), middle1 = VRIK_LadderFingerBone(side, 7),
              ring1 = VRIK_LadderFingerBone(side, 8), pinky1 = VRIK_LadderFingerBone(side, 9),
              middle2 = VRIK_LadderFingerBone(side, 12);
    for (int b : {index1, middle1, ring1, pinky1, middle2, slotBone})
        if (!ValidBone(b)) return false;
    Pose i1, m1, r1, p1;
    if (!InFrame(buf, side, index1, hand, ref, i1) || !InFrame(buf, side, middle1, hand, ref, m1) ||
        !InFrame(buf, side, ring1, hand, ref, r1) || !InFrame(buf, side, pinky1, hand, ref, p1) ||
        !SlotFrame(buf, side, hand, slotBone, ref, pm.slot))
        return false;
    for (int k = 0; k < 3; ++k) pm.knuckle[k] = 0.25f * (i1.p[k] + m1.p[k] + r1.p[k] + p1.p[k]);
    std::copy_n(i1.p, 3, pm.index);
    std::copy_n(m1.p, 3, pm.d);
    if (!Unit(pm.d)) return false;
    float across[3];
    Sub(i1.p, p1.p, across);
    const float width = Len(across);
    Mad(across, pm.d, -Dot(across, pm.d), pm.t);
    if (!Unit(pm.t)) return false;
    Cross(pm.d, pm.t, pm.n);
    if (!Unit(pm.n)) return false;
    // The side the fingers close to: the middle finger's proximal phalanx turned about its own -Z (the rig's
    // flexion on both hands -- the ladder grips, cvr::ladder::profile) takes its next joint there. The hand's
    // handedness says the same (right: -(d x t)); a disagreement is counted, the flexion wins.
    Pose next = Local(buf, middle2);
    const float z[3] = {0.0f, 0.0f, -1.0f};
    float bend[4], turned[4], a[3], b[3], moved[3];
    AxisAngle(z, 20.0f * kDeg, bend);
    VRIK_QuatMul(m1.q, bend, turned);
    Rot(m1.q, next.p, a);
    Rot(turned, next.p, b);
    Sub(b, a, moved);
    const bool flexion = Dot(moved, pm.n) >= 0.0f;
    if (!flexion) for (float& v : pm.n) v = -v;
    CyberpunkVR_DebugGraspNormalFlip[side] = (flexion == (side == 1)) ? 1 : 0;
    for (int k = 0; k < 3; ++k) pm.base[k] = 0.5f * pm.knuckle[k];
    pm.halfD = 0.5f * Dot(pm.knuckle, pm.d);
    pm.halfT = 0.5f * width + 0.008f;
    Mad(pm.base, pm.n, kPalmSkin, pm.skin);
    // The pinch: the index finger bent a little, its pad -- the distal phalanx's middle on the side it bends to.
    ChainBones c;
    const int* slots = kFingers[0].slot;
    float J[4][3], hinge[3][3], rot[3][4];
    Pose F[3];
    if (!ChainOf(buf, side, hand, slots, ref, c)) return false;
    for (int j = 0; j < 3; ++j) std::copy_n(ref[slots[j]], 4, rot[j]);
    Joints(c, rot, J, F);
    if (!Hinges(J, F, pm.n, hinge)) return false;
    for (int j = 0; j < 3; ++j) {
        float turn[4];
        AxisAngle(hinge[j], kPinchBend[j], turn);
        VRIK_QuatMul(ref[slots[j]], turn, rot[j]);
        VRIK_QuatNorm(rot[j]);
    }
    Joints(c, rot, J, F);
    float seg[3], axis[3];
    Sub(J[3], J[2], seg);
    Rot(F[2].q, hinge[2], axis);
    Cross(axis, seg, pm.pinchN);
    if (!Unit(pm.pinchN)) return false;
    for (int k = 0; k < 3; ++k) pm.pinch[k] = J[2][k] + 0.55f * seg[k];
    Mad(pm.pinch, pm.pinchN, kPinchPad, pm.pinch);
    return pm.halfD > 0.01f;
}

// ---------------------------------------------------------------- the state

struct Request { int shape = kBox; float half[3]{}; Pose obj; };

struct Side {
    std::mutex lock;
    bool active = false;
    unsigned rev = 0, solved = ~0u;
    Request req;
    float blend = 0.0f;
    float rot[kSlots][4]{};
    bool have[kSlots]{};
    bool palm = false;
    Palm pm{};
};
Side g_side[2];

// The thing in the hand's frame, ready for distances.
struct Thing {
    int shape;
    float half[3];
    Pose pose;
    float Dist(const float* p) const {
        float l[3];
        ToFrame(pose, p, l);
        return Sdf(shape, half, l);
    }
    void Grad(const float* p, float* g) const {
        const float e = 0.0005f;
        for (int k = 0; k < 3; ++k) {
            float a[3] = {p[0], p[1], p[2]}, b[3] = {p[0], p[1], p[2]};
            a[k] += e;
            b[k] -= e;
            g[k] = Dist(a) - Dist(b);
        }
        if (!Unit(g)) { g[0] = g[1] = 0.0f; g[2] = 1.0f; }
    }
    // The surface point nearest p.
    void Nearest(const float* p, float* o) const {
        std::copy_n(p, 3, o);
        for (int i = 0; i < 4; ++i) {
            float g[3];
            Grad(o, g);
            Mad(o, g, -Dist(o), o);
        }
    }
};

float PalmDist(const Palm& pm, const float* p) {
    float rel[3];
    Sub(p, pm.base, rel);
    const float q[3] = {std::fabs(Dot(rel, pm.d)) - pm.halfD, std::fabs(Dot(rel, pm.t)) - pm.halfT,
                        std::fabs(Dot(rel, pm.n)) - kPalmSkin};
    const float o[3] = {std::max(q[0], 0.0f), std::max(q[1], 0.0f), std::max(q[2], 0.0f)};
    return Len(o) + std::min(std::max(q[0], std::max(q[1], q[2])), 0.0f);
}

// A closed finger: its joints and tip (hand frame) and its capsules' radii.
struct Closed { float J[4][3]; float radius[3]; bool ok = false; };

// One finger closed: its three bones' rotations out of `start`, written to `out`; its angles (degrees) to `deg`.
int CloseFinger(const uint8_t* buf, int side, int hand, const Finger& f, const Thing& th, const Palm& pm,
                const float (*start)[4], float limit, float spread, float (*out)[4], float* deg, Closed& closed) {
    ChainBones c;
    if (!ChainOf(buf, side, hand, f.slot, start, c)) return 0;
    float rot0[3][4], rot[3][4], J[4][3];
    Pose F[3];
    for (int j = 0; j < 3; ++j) std::copy_n(c.local[j].q, 4, rot0[j]);
    Joints(c, rot0, J, F);
    // Spread: the proximal phalanx turned in the palm's plane, about the bone axis nearest the palm's normal.
    if (spread != 0.0f) {
        float axis[3], turn[4], m[4];
        LocalHinge(F[0].q, pm.n, axis);
        AxisAngle(axis, spread, turn);
        VRIK_QuatMul(rot0[0], turn, m);
        VRIK_QuatNorm(m);
        std::copy_n(m, 4, rot0[0]);
        Joints(c, rot0, J, F);
    }
    float hinge[3][3];
    if (!Hinges(J, F, pm.n, hinge)) return 0;
    float theta[3] = {}, prev[3] = {}, maxRad[3];
    bool frozen[3] = {};
    for (int j = 0; j < 3; ++j) maxRad[j] = f.maxDeg[j] * kDeg * limit;
    auto pose = [&](const float* t) {
        for (int j = 0; j < 3; ++j) {
            float turn[4];
            AxisAngle(hinge[j], t[j], turn);
            VRIK_QuatMul(rot0[j], turn, rot[j]);
            VRIK_QuatNorm(rot[j]);
        }
    };
    // A segment's clearance: from the thing, and (but the first segment) from the palm.
    auto clearance = [&](int s, float* obj, float* palm) {
        *obj = *palm = 1e9f;
        for (float u : {0.25f, 0.5f, 0.75f, 1.0f}) {
            float p[3];
            for (int k = 0; k < 3; ++k) p[k] = J[s][k] + (J[s + 1][k] - J[s][k]) * u;
            *obj = std::min(*obj, th.Dist(p) - f.radius[s]);
            if (s > 0) *palm = std::min(*palm, PalmDist(pm, p) - f.radius[s]);
        }
    };
    float lastObj[3], lastPalm[3];
    for (int s = 0; s < 3; ++s) clearance(s, &lastObj[s], &lastPalm[s]);
    int contacts = 0;
    for (int step = 0; step < 240; ++step) {
        std::copy_n(theta, 3, prev);
        bool moved = false;
        for (int j = 0; j < 3; ++j) {
            if (frozen[j] || theta[j] >= maxRad[j]) continue;
            // A joint leads once the one before it is done; until then it only follows a little.
            const bool leads = j == 0 || frozen[j - 1] || theta[j - 1] >= maxRad[j - 1];
            theta[j] = std::min(maxRad[j], theta[j] + kStep * f.speed[j] * (leads ? 1.0f : kFollow));
            moved = true;
        }
        if (!moved) break;
        pose(theta);
        Joints(c, rot, J, F);
        for (int s = 0; s < 3; ++s) {
            float o, p;
            clearance(s, &o, &p);
            // Into the thing or the palm: a segment that was clear, or one already in that goes deeper.
            const bool hit = (o < 0.0f && o < lastObj[s] - 1e-5f) || (p < 0.0f && p < lastPalm[s] - 1e-5f);
            lastObj[s] = o;
            lastPalm[s] = p;
            if (!hit) continue;
            for (int j = 0; j <= s; ++j) {
                if (frozen[j]) continue;
                theta[j] = prev[j];
                frozen[j] = true;
            }
            ++contacts;
        }
    }
    pose(theta);
    Joints(c, rot, J, F);
    for (int j = 0; j < 3; ++j) {
        std::copy_n(rot[j], 4, out[j]);
        deg[j] = theta[j] / kDeg;
        closed.radius[j] = f.radius[j];
    }
    std::copy_n(&J[0][0], 12, &closed.J[0][0]);
    closed.ok = true;
    return contacts;
}

// The thumb: its metacarpal turned in front of the palm (lift) and across it (swing), about the palm's own axes at its
// base, a little rolled with the lift (opposition turns the pad to the fingers), and its two joints bent about their
// hinges. Searched -- a coarse grid, then finer steps one angle at a time -- for the least cost: the thumb out of the
// thing, the palm and the closed fingers; its pad on the thing's surface, near the grasp's goal and facing it.
struct ThumbAngles { float lift, swing, roll, mcp, ip; };

bool SolveThumb(const uint8_t* buf, int side, int hand, Kind kind, int axis, const Thing& th, const Palm& pm,
                const float (*start)[4], const Closed* fingers, float (*out)[4], float* deg) {
    ChainBones c;
    if (!ChainOf(buf, side, hand, kThumbSlots, start, c)) return false;
    float rot0[3][4], J[4][3];
    Pose F[3];
    for (int j = 0; j < 3; ++j) std::copy_n(c.local[j].q, 4, rot0[j]);
    Joints(c, rot0, J, F);
    float toward[3];
    Sub(pm.skin, J[1], toward);
    float hinge[3][3];
    if (!Hinges(J, F, toward, hinge)) return false;
    // The palm's axes for the base: lift takes the thumb (lying toward t) to the palm's front, swing toward the
    // fingertips, roll about the metacarpal itself.
    float lift[3], swing[3], dxt[3];
    Cross(pm.d, pm.t, dxt);
    const bool front = Dot(dxt, pm.n) > 0.0f;
    for (int k = 0; k < 3; ++k) {
        lift[k] = front ? pm.d[k] : -pm.d[k];
        swing[k] = front ? -pm.n[k] : pm.n[k];
    }
    float meta[3];
    Sub(J[1], J[0], meta);
    Unit(meta);
    float baseModel[4];
    VRIK_QuatMul(c.base.q, rot0[0], baseModel);
    float baseInv[4];
    VRIK_QuatConj(c.base.q, baseInv);
    // The goal for the pad.
    float goal[3];
    switch (kind) {
    case kWrap: {
        // Round its side the other way from the fingers (a large-diameter wrap, Feix et al. 2016 #1): on the
        // cylinder's back and top as the palm sees it, where the thumb meets the fingertips -- at the index finger's
        // place along the axis, a little toward the thumb. Not on its end.
        const float e[3] = {axis == 0 ? 1.0f : 0.0f, axis == 1 ? 1.0f : 0.0f, axis == 2 ? 1.0f : 0.0f};
        float a[3], rel[3], q[3], u[3], p[3];
        Rot(th.pose.q, e, a);
        Sub(pm.index, th.pose.p, rel);
        const float length = th.shape == kBox ? th.half[axis] : th.half[2];
        const float along = std::clamp(Dot(rel, a) + 0.01f * (Dot(a, pm.t) >= 0.0f ? 1.0f : -1.0f),
                                       -0.8f * length, 0.8f * length);
        Mad(th.pose.p, a, along, q);
        for (int k = 0; k < 3; ++k) u[k] = pm.n[k] * 0.75f - pm.d[k] * 0.65f;
        Mad(u, a, -Dot(u, a), u);
        if (!Unit(u)) std::copy_n(pm.n, 3, u);
        Mad(q, u, 0.2f, p);
        th.Nearest(p, goal);
        break;
    }
    case kBall: {
        float dir[3];
        for (int k = 0; k < 3; ++k) dir[k] = pm.t[k] * 0.75f + pm.n[k] * 0.6f;
        Unit(dir);
        float p[3];
        Mad(th.pose.p, dir, 1.0f, p);
        th.Nearest(p, goal);
        break;
    }
    case kPinch: {
        float p[3];
        Mad(th.pose.p, pm.pinchN, 0.2f, p);
        th.Nearest(p, goal);
        break;
    }
    case kEdge: {
        // The other cover, toward the wrist, a little out of the palm by the index finger.
        float p[3];
        Mad(pm.index, pm.n, 0.04f, p);
        Mad(p, pm.d, -0.05f, p);
        th.Nearest(p, goal);
        break;
    }
    default: {
        // Beside the index finger's knuckle, on the thumb's side: a big box's face, a narrow one's side.
        float p[3];
        Mad(pm.index, pm.t, 0.025f, p);
        Mad(p, pm.n, kPalmSkin, p);
        th.Nearest(p, goal);
        break;
    }
    }
    float penOut = 0.0f, gapOut = 0.0f, goalOut = 0.0f;
    auto cost = [&](const ThumbAngles& a, float (*rot)[4]) {
        float qa[4], qs[4], qr[4], m[4], m2[4];
        AxisAngle(lift, a.lift, qa);
        AxisAngle(swing, a.swing, qs);
        AxisAngle(meta, a.roll, qr);
        VRIK_QuatMul(qa, qr, m);
        VRIK_QuatMul(qs, m, m2);
        VRIK_QuatMul(m2, baseModel, m);
        VRIK_QuatMul(baseInv, m, rot[0]);
        VRIK_QuatNorm(rot[0]);
        float t1[4], t2[4];
        AxisAngle(hinge[1], a.mcp, t1);
        AxisAngle(hinge[2], a.ip, t2);
        VRIK_QuatMul(rot0[1], t1, rot[1]);
        VRIK_QuatMul(rot0[2], t2, rot[2]);
        VRIK_QuatNorm(rot[1]);
        VRIK_QuatNorm(rot[2]);
        float P[4][3];
        Pose G[3];
        Joints(c, rot, P, G);
        float pen = 0.0f;
        for (int s = 0; s < 3; ++s) {
            // The metacarpal is in the thenar's flesh -- the palm itself: only the joints after it are kept out.
            if (s == 0) continue;
            for (float u : {0.0f, 0.25f, 0.5f, 0.75f, 1.0f}) {
                if (s == 1 && u < 0.25f) continue;
                float p[3];
                for (int k = 0; k < 3; ++k) p[k] = P[s][k] + (P[s + 1][k] - P[s][k]) * u;
                const float o = kThumbRadius[s] - th.Dist(p);
                if (o > 0.0f) pen += o * o;
                const float q = kThumbRadius[s] - PalmDist(pm, p);
                if (q > 0.0f) pen += q * q;
            }
            for (int i = 0; i < 4; ++i) {
                if (!fingers[i].ok) continue;
                for (int g = 0; g < 3; ++g) {
                    const float o = kThumbRadius[s] + fingers[i].radius[g] -
                                    SegSeg(P[s], P[s + 1], fingers[i].J[g], fingers[i].J[g + 1]);
                    if (o > 0.0f) pen += o * o;
                }
            }
        }
        float pad[3], seg[3];
        Sub(P[3], P[2], seg);
        Mad(P[2], seg, 0.7f, pad);
        const float gap = th.Dist(pad) - kThumbRadius[2];
        float off[3];
        Sub(pad, goal, off);
        // The pad facing the thing: the way the distal joint bends it, against the surface's normal.
        float axis[3], padN[3], g[3];
        Rot(G[2].q, hinge[2], axis);
        Cross(axis, seg, padN);
        Unit(padN);
        th.Grad(pad, g);
        const float facing = 1.0f + Dot(padN, g);
        const float reg = a.lift * a.lift + a.swing * a.swing + a.roll * a.roll + a.mcp * a.mcp + a.ip * a.ip;
        penOut = std::sqrt(pen);
        gapOut = gap;
        goalOut = Len(off);
        return 1e5f * pen + 2e3f * std::max(gap, 0.0f) * std::max(gap, 0.0f) + 30.0f * Dot(off, off) +
               0.01f * facing + 0.002f * reg;
    };
    const float lo[5] = {-40.0f * kDeg, -40.0f * kDeg, -30.0f * kDeg, -5.0f * kDeg, -15.0f * kDeg};
    const float hi[5] = {100.0f * kDeg, 70.0f * kDeg, 60.0f * kDeg, 60.0f * kDeg, 85.0f * kDeg};
    auto get = [](const ThumbAngles& a, int i) -> float {
        const float v[5] = {a.lift, a.swing, a.roll, a.mcp, a.ip};
        return v[i];
    };
    auto set = [](ThumbAngles& a, int i, float v) {
        float* f[5] = {&a.lift, &a.swing, &a.roll, &a.mcp, &a.ip};
        *f[i] = v;
    };
    float rot[3][4];
    ThumbAngles best{0, 0, 0, 0, 0};
    float bestCost = cost(best, rot);
    for (float l = lo[0]; l <= hi[0] + 1e-4f; l += 15.0f * kDeg)
        for (float s = lo[1]; s <= hi[1] + 1e-4f; s += 15.0f * kDeg)
            for (float m = 0.0f; m <= hi[3] + 1e-4f; m += 20.0f * kDeg)
                for (float i = 0.0f; i <= hi[4] + 1e-4f; i += 20.0f * kDeg) {
                    const ThumbAngles a{l, s, std::clamp(0.4f * l, lo[2], hi[2]), m, i};
                    const float v = cost(a, rot);
                    if (v < bestCost) { bestCost = v; best = a; }
                }
    for (float step : {8.0f * kDeg, 4.0f * kDeg, 2.0f * kDeg, 1.0f * kDeg}) {
        for (int it = 0; it < 24; ++it) {
            bool better = false;
            for (int k = 0; k < 5; ++k)
                for (float sgn : {-1.0f, 1.0f}) {
                    ThumbAngles a = best;
                    set(a, k, std::clamp(get(a, k) + sgn * step, lo[k], hi[k]));
                    const float v = cost(a, rot);
                    if (v < bestCost - 1e-7f) { bestCost = v; best = a; better = true; }
                }
            if (!better) break;
        }
    }
    const float v = cost(best, out);
    CyberpunkVR_DebugGraspThumb[side][0] = v;
    CyberpunkVR_DebugGraspThumb[side][1] = penOut * 1000.0f;
    CyberpunkVR_DebugGraspThumb[side][2] = gapOut * 1000.0f;
    CyberpunkVR_DebugGraspThumb[side][3] = goalOut * 1000.0f;
    deg[0] = best.lift / kDeg;
    deg[1] = best.swing / kDeg;
    deg[2] = best.mcp / kDeg;
    deg[3] = best.ip / kDeg;
    return true;
}

// The fingers' spread for a ball: wider the bigger it is (radians, the index finger's; the others by their share).
float SpreadFor(Kind kind, const float* half) {
    if (kind == kBall) return std::clamp(half[0] * 1.6f, 0.08f, 0.22f);
    if (kind == kFlat) return 0.05f;
    return 0.0f;
}

void Solve(const uint8_t* buf, int side, int hand, Side& S) {
    float start[kSlots][4];
    ReferenceFingers(buf, side, start, S.have);
    for (int s = 0; s < kSlots; ++s) std::copy_n(start[s], 4, S.rot[s]);
    int axis;
    const Kind kind = Classify(S.req.shape, S.req.half, axis);
    Thing th{S.req.shape, {S.req.half[0], S.req.half[1], S.req.half[2]}, Compose(S.pm.slot, S.req.obj)};
    // Spread toward the thumb (index) or away from it (ring, pinky), whichever way that turns about the normal.
    float nxd[3];
    Cross(S.pm.n, S.pm.d, nxd);
    const float toThumb = Dot(nxd, S.pm.t) >= 0.0f ? 1.0f : -1.0f;
    const float spread = SpreadFor(kind, S.req.half);
    int contacts = 0;
    Closed closed[4];
    for (int i = 0; i < 4; ++i) {
        const Finger& f = kFingers[i];
        const float limit = (kind == kPinch && i >= 1) ? 0.6f : 1.0f;
        float out[3][4], deg[3] = {};
        const int got = CloseFinger(buf, side, hand, f, th, S.pm, start, limit, f.spread * spread * toThumb, out, deg,
                                    closed[i]);
        if (!closed[i].ok) continue;
        contacts += got;
        for (int j = 0; j < 3; ++j) {
            std::copy_n(out[j], 4, S.rot[f.slot[j]]);
            CyberpunkVR_DebugGraspAngles[side][5 + i * 3 + j] = deg[j];
        }
    }
    float thumb[3][4], deg[4] = {};
    if (SolveThumb(buf, side, hand, kind, axis, th, S.pm, start, closed, thumb, deg)) {
        for (int j = 0; j < 3; ++j) std::copy_n(thumb[j], 4, S.rot[kThumbSlots[j]]);
        for (int j = 0; j < 4; ++j) CyberpunkVR_DebugGraspAngles[side][1 + j] = deg[j];
    }
    CyberpunkVR_DebugGraspAngles[side][0] = static_cast<float>(kind);
    CyberpunkVR_DebugGrasp[3] = static_cast<uint64_t>(contacts);
    ++CyberpunkVR_DebugGrasp[0];
}

// The seat of a thing in the slot's frame (see the header): `p`, `q` where it is in the slot when the hand takes it.
bool Seat(int side, int shape, const float* half, const float* p, const float* q, Pose& out) {
    Side& S = g_side[side];
    std::lock_guard<std::mutex> lk(S.lock);
    if (!S.palm) return false;
    const Palm& pm = S.pm;
    int axis;
    const Kind kind = Classify(shape, half, axis);
    Pose item;
    std::copy_n(p, 3, item.p);
    std::copy_n(q, 4, item.q);
    VRIK_QuatNorm(item.q);
    const Pose held = Compose(pm.slot, item);   // in the hand's frame
    float r[4];
    std::copy_n(held.q, 4, r);
    auto turnOnto = [&](const float* from, const float* to) {
        float turn[4], next[4];
        VRIK_QuatFromTo(from, to, turn);
        VRIK_QuatMul(turn, r, next);
        VRIK_QuatNorm(next);
        std::copy_n(next, 4, r);
    };
    auto axisOf = [&](int k, float* o) {
        const float e[3] = {k == 0 ? 1.0f : 0.0f, k == 1 ? 1.0f : 0.0f, k == 2 ? 1.0f : 0.0f};
        Rot(r, e, o);
    };
    // Turned about `about` (unit), the least, so that axis k lies along +-target projected off `about`.
    auto alignAbout = [&](int k, const float* about, const float* target) {
        float a[3], g[3];
        axisOf(k, a);
        Mad(a, about, -Dot(a, about), a);
        Mad(target, about, -Dot(target, about), g);
        if (!Unit(a) || !Unit(g)) return;
        if (Dot(a, g) < 0.0f) for (float& v : g) v = -v;
        turnOnto(a, g);
    };
    // Turned about `about`, the least, so that one of the axes but `skip` lies along one of the targets.
    auto edgeOnto = [&](int skip, const float* about, const float* t1, const float* t2) {
        float best = -1.0f, from[3] = {}, onto[3] = {};
        for (int k = 0; k < 3; ++k) {
            if (k == skip) continue;
            float a[3];
            axisOf(k, a);
            Mad(a, about, -Dot(a, about), a);
            if (!Unit(a)) continue;
            for (const float* target : {t1, t2}) {
                float g[3];
                Mad(target, about, -Dot(target, about), g);
                if (!Unit(g)) continue;
                const float c = Dot(a, g);
                if (std::fabs(c) <= best) continue;
                best = std::fabs(c);
                std::copy_n(a, 3, from);
                for (int i = 0; i < 3; ++i) onto[i] = c >= 0.0f ? g[i] : -g[i];
            }
        }
        if (best >= 0.0f) turnOnto(from, onto);
    };
    float pos[3];
    switch (kind) {
    case kWrap: {
        float w[3];
        for (int k = 0; k < 3; ++k) w[k] = pm.t[k] * std::cos(kWrapTilt) + pm.d[k] * std::sin(kWrapTilt);
        Unit(w);
        float a[3], to[3];
        axisOf(axis, a);
        for (int k = 0; k < 3; ++k) to[k] = Dot(a, w) >= 0.0f ? w[k] : -w[k];
        turnOnto(a, to);
        if (shape == kBox) {
            // A bar's flat side on the palm: its thinner cross axis onto the normal, about its length.
            int thin = -1;
            for (int k = 0; k < 3; ++k)
                if (k != axis && (thin < 0 || half[k] < half[thin])) thin = k;
            alignAbout(thin, to, pm.n);
        }
        // A can's axis a little behind the knuckles; a flat bar's front edge just behind them, so the fingers fold
        // over it from the knuckles.
        float c[3], rel[3];
        const float back = shape == kBox ? std::max(kWrapBack, Support(shape, half, r, pm.d) - 0.004f) : kWrapBack;
        Mad(pm.skin, pm.d, pm.halfD - back, c);
        Sub(held.p, c, rel);
        const float length = shape == kBox ? half[axis] : half[2];
        const float room = std::max(0.0f, length - 0.035f);
        float along = std::clamp(Dot(rel, w), -room, room);
        // Its end on the thumb's side out past the index finger, so that the thumb closes round its side and not over
        // its end: a can stands out of the top of the hand.
        float idx[3];
        Sub(pm.index, c, idx);
        along = std::max(along, std::min(Dot(idx, w) + kWrapOverhang - length, length));
        Mad(c, w, along, pos);
        Mad(pos, pm.n, Support(shape, half, r, pm.n), pos);
        break;
    }
    case kEdge: {
        // The thin axis along the fingers, the longest out of the palm (the short edge across it), the least turns;
        // a cover a little behind the knuckles, the edge on the palm, its middle in the palm's middle.
        float a[3], to[3];
        axisOf(axis, a);
        for (int k = 0; k < 3; ++k) to[k] = Dot(a, pm.d) >= 0.0f ? pm.d[k] : -pm.d[k];
        turnOnto(a, to);
        int longest = -1;
        for (int k = 0; k < 3; ++k)
            if (k != axis && (longest < 0 || half[k] > half[longest])) longest = k;
        alignAbout(longest, to, pm.n);
        Mad(pm.skin, pm.d, pm.halfD - kEdgeGap - Support(shape, half, r, pm.d), pos);
        Mad(pos, pm.n, Support(shape, half, r, pm.n), pos);
        break;
    }
    case kBall: {
        const float radius = half[0];
        Mad(pm.skin, pm.d, radius < kWrapRadius ? pm.halfD - kWrapBack : 0.008f, pos);
        Mad(pos, pm.n, radius + (radius < kWrapRadius ? 0.0f : kPadRise), pos);
        break;
    }
    case kPinch: {
        int thin = 0;
        for (int k = 1; k < 3; ++k) if (half[k] < half[thin]) thin = k;
        float a[3], to[3];
        axisOf(thin, a);
        for (int k = 0; k < 3; ++k) to[k] = Dot(a, pm.pinchN) >= 0.0f ? pm.pinchN[k] : -pm.pinchN[k];
        turnOnto(a, to);
        edgeOnto(thin, pm.pinchN, pm.d, pm.t);
        Mad(pm.pinch, pm.pinchN, Support(shape, half, r, pm.pinchN), pos);
        break;
    }
    default: {
        // The face most turned to the palm onto it, the least turn; then an edge along the fingers or across them.
        float best[3] = {};
        int face = 0;
        for (int k = 0; k < 3; ++k) {
            float a[3];
            axisOf(k, a);
            if (k == 0 || std::fabs(Dot(a, pm.n)) > std::fabs(Dot(best, pm.n))) { face = k; std::copy_n(a, 3, best); }
        }
        float to[3];
        for (int k = 0; k < 3; ++k) to[k] = Dot(best, pm.n) >= 0.0f ? pm.n[k] : -pm.n[k];
        turnOnto(best, to);
        edgeOnto(face, pm.n, pm.d, pm.t);
        // Where the hand took it, the palm's centre kept on the face.
        float rel[3];
        Sub(held.p, pm.skin, rel);
        const float roomD = std::max(0.0f, Support(shape, half, r, pm.d) - kFlatMargin);
        const float roomT = std::max(0.0f, Support(shape, half, r, pm.t) - kFlatMargin);
        Mad(pm.skin, pm.d, std::clamp(Dot(rel, pm.d), -roomD, roomD), pos);
        Mad(pos, pm.t, std::clamp(Dot(rel, pm.t), -roomT, roomT), pos);
        Mad(pos, pm.n, Support(shape, half, r, pm.n) + kPadRise, pos);
        break;
    }
    }
    ToFrame(pm.slot, pos, out.p);
    float inv[4];
    VRIK_QuatConj(pm.slot.q, inv);
    VRIK_QuatMul(inv, r, out.q);
    VRIK_QuatNorm(out.q);
    return true;
}

// The slot a held thing hangs from back at the rig's reference pose under the hand (see the header).
void PinSlot(uint8_t* buf, int slot) {
    float p[3], q[4];
    if (!VRIK_ReferenceLocal(slot, p, q)) return;
    std::memcpy(buf + slot * 48 + VRIK_TRANS_OFF, p, sizeof(p));
    std::memcpy(buf + slot * 48 + VRIK_ROT_OFF, q, sizeof(q));
}

}  // namespace

namespace cvr::anim {

void GraspFingers(uint8_t* buf) {
    if (!buf || g_VRBoneCount <= 0) return;
    for (int side = 0; side < 2; ++side) {
        const int hand = side ? g_VRRightBoneIdx : g_VRLeftBoneIdx;
        const int slotBone = side ? g_VRSmokeCigIdx : g_VRSmokeLighterIdx;
        if (!ValidBone(hand) || !ValidBone(slotBone)) continue;
        Side& S = g_side[side];
        bool holding;
        {
            std::lock_guard<std::mutex> lk(S.lock);
            holding = S.active;
        }
        if (holding) PinSlot(buf, slotBone);
        Palm pm{};
        if (!MeasurePalm(buf, side, hand, slotBone, pm)) continue;
        std::lock_guard<std::mutex> lk(S.lock);
        S.pm = pm;
        S.palm = true;
        ++CyberpunkVR_DebugGrasp[2];
        {
            float v[4][3];
            ToFrame(pm.slot, pm.skin, v[0]);
            Unrot(pm.slot.q, pm.n, v[1]);
            Unrot(pm.slot.q, pm.d, v[2]);
            Unrot(pm.slot.q, pm.t, v[3]);
            for (int i = 0; i < 4; ++i)
                for (int k = 0; k < 3; ++k) CyberpunkVR_DebugGraspPalm[side][i * 3 + k] = v[i][k];
        }
        if (!S.active) continue;
        if (S.solved != S.rev) {
            Solve(buf, side, hand, S);
            S.solved = S.rev;
        }
        const float b = S.blend;
        if (b <= 0.001f) continue;
        for (int s = 0; s < kSlots; ++s) {
            if (!S.have[s]) continue;
            const int bone = VRIK_LadderFingerBone(side, s);
            if (!ValidBone(bone)) continue;
            float* q = reinterpret_cast<float*>(buf + bone * 48 + VRIK_ROT_OFF);
            const float* t = S.rot[s];
            if (b >= 0.999f) {
                std::copy_n(t, 4, q);
                continue;
            }
            const float dot = q[0] * t[0] + q[1] * t[1] + q[2] * t[2] + q[3] * t[3];
            const float w = dot < 0.0f ? -b : b;
            float m[4];
            for (int k = 0; k < 4; ++k) m[k] = q[k] * (1.0f - b) + t[k] * w;
            VRIK_QuatNorm(m);
            std::copy_n(m, 4, q);
        }
        ++CyberpunkVR_DebugGrasp[1];
    }
}

bool GraspHolding(int side) {
    if (side < 0 || side > 1) return false;
    Side& S = g_side[side];
    std::lock_guard<std::mutex> lk(S.lock);
    return S.active;
}

}  // namespace cvr::anim

// ---------------------------------------------------------------- natives (side: 0 left, 1 right)

// VRGraspSeat(side, shape, hx, hy, hz, position, orientation) -> Transform: where a thing of that shape, at that
// pose in the slot when the hand takes it, lies for its grasp, in the slot's frame. position.W = 1 when seated, 0
// when the palm is not measured yet.
void VRGraspSeat(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, RED4ext::Transform* aOut, int64_t) {
    int32_t side = -1, shape = kBox;
    float h[3]{};
    RED4ext::Vector4 p{};
    RED4ext::Quaternion q{};
    RED4ext::GetParameter(aFrame, &side);
    RED4ext::GetParameter(aFrame, &shape);
    RED4ext::GetParameter(aFrame, &h[0]);
    RED4ext::GetParameter(aFrame, &h[1]);
    RED4ext::GetParameter(aFrame, &h[2]);
    RED4ext::GetParameter(aFrame, &p);
    RED4ext::GetParameter(aFrame, &q);
    aFrame->code++;
    if (!aOut) return;
    aOut->position.X = aOut->position.Y = aOut->position.Z = aOut->position.W = 0.0f;
    aOut->orientation = q;
    if (side < 0 || side > 1 || shape < kBox || shape > kCone) return;
    const float at[3] = {p.X, p.Y, p.Z};
    const float in[4] = {q.i, q.j, q.k, q.r};
    Pose seat;
    if (!Seat(side, shape, h, at, in, seat)) return;
    aOut->position.X = seat.p[0];
    aOut->position.Y = seat.p[1];
    aOut->position.Z = seat.p[2];
    aOut->position.W = 1.0f;
    aOut->orientation.i = seat.q[0];
    aOut->orientation.j = seat.q[1];
    aOut->orientation.k = seat.q[2];
    aOut->orientation.r = seat.q[3];
}

// VRGraspSet(side, shape, hx, hy, hz, position, orientation) -> Bool: the hand holds that thing there (slot frame);
// its fingers are closed on it from the next pass.
void VRGraspSet(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t) {
    int32_t side = -1, shape = kBox;
    float h[3]{};
    RED4ext::Vector4 p{};
    RED4ext::Quaternion q{};
    RED4ext::GetParameter(aFrame, &side);
    RED4ext::GetParameter(aFrame, &shape);
    RED4ext::GetParameter(aFrame, &h[0]);
    RED4ext::GetParameter(aFrame, &h[1]);
    RED4ext::GetParameter(aFrame, &h[2]);
    RED4ext::GetParameter(aFrame, &p);
    RED4ext::GetParameter(aFrame, &q);
    aFrame->code++;
    if (aOut) *aOut = false;
    if (side < 0 || side > 1 || shape < kBox || shape > kCone) return;
    Side& S = g_side[side];
    std::lock_guard<std::mutex> lk(S.lock);
    S.req.shape = shape;
    std::copy_n(h, 3, S.req.half);
    S.req.obj.p[0] = p.X; S.req.obj.p[1] = p.Y; S.req.obj.p[2] = p.Z;
    S.req.obj.q[0] = q.i; S.req.obj.q[1] = q.j; S.req.obj.q[2] = q.k; S.req.obj.q[3] = q.r;
    VRIK_QuatNorm(S.req.obj.q);
    S.active = true;
    ++S.rev;
    if (aOut) *aOut = true;
}

// VRGraspBlend(side, blend) -> Bool: how far the fingers are on the grasp (0 tracking, 1 the grasp).
void VRGraspBlend(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t) {
    int32_t side = -1;
    float b = 0.0f;
    RED4ext::GetParameter(aFrame, &side);
    RED4ext::GetParameter(aFrame, &b);
    aFrame->code++;
    if (aOut) *aOut = false;
    if (side < 0 || side > 1) return;
    Side& S = g_side[side];
    std::lock_guard<std::mutex> lk(S.lock);
    S.blend = std::clamp(b, 0.0f, 1.0f);
    if (aOut) *aOut = true;
}

// VRGraspClear(side) -> Bool: the hand holds nothing; its fingers are the tracking's again.
void VRGraspClear(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t) {
    int32_t side = -1;
    RED4ext::GetParameter(aFrame, &side);
    aFrame->code++;
    if (aOut) *aOut = false;
    if (side < 0 || side > 1) return;
    Side& S = g_side[side];
    std::lock_guard<std::mutex> lk(S.lock);
    S.active = false;
    S.blend = 0.0f;
    if (aOut) *aOut = true;
}
