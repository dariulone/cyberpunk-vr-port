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
//  - PINCH: a tiny thing (no half size of 1.2 cm): flat on the pad of the index finger bent a little, the thumb's pad
//    on its other side;
//  - TRIPOD: a small thing (a car key's fob: no half size of 4.5 cm, the middle one under 2.2 cm): between the thumb's
//    pad and the pads of the index and middle fingers side by side, its long side across the two; ring and little
//    finger curled out of the way. A plate too thick for a cover's grip (4-6 cm: a figure's body) the same way by its
//    side, like a crab's claw: near its edge, the rest of it out of the hand.
// A long thing (a can, a bottle, a bar) is taken the way the hand came to it (Approach), not by its shape alone:
//  - in its middle, or from its side: WRAP;
//  - TOP: the palm over one of its ends, facing along it, that end not the lower one: the end a little under the palm,
//    the fingertips round its rim and the thumb on the other side -- the palm does not touch it;
//  - CRADLE: the palm under its lower end, facing up along it (caught from below): it stands on the palm, the fingers
//    up its sides.
// Which end is lower is the world's: the model's up in the hand's frame, from the drawn hand of the frame.
// A PART of something bigger (GraspPartOf: a bottle's neck, or its body below the shoulder) is wrapped however thin it
// is -- a bottle by its neck is held in the fist, not in three fingers -- and only its free ends are taken from above
// or below: a bottle stands on its base on a palm, never on its neck, and no palm is put over its shoulder.
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
#include <chrono>
#include <cmath>
#include <cstring>
#include <mutex>

extern void Log(const char* fmt, ...);

extern "C" {
// Solves, passes that wrote a grasp, palms measured, segments that touched in the last solve.
__declspec(dllexport) volatile uint64_t CyberpunkVR_DebugGrasp[4] = {};
// Per side, the last solve: its kind (0 flat, 1 wrap, 2 ball, 3 pinch, 4 edge, 5 top, 6 cradle, 7 tripod), the thumb's metacarpal elevation and angle out and its two
// joints, then index..pinky's three joints each, in degrees.
__declspec(dllexport) volatile float CyberpunkVR_DebugGraspAngles[2][17] = {};
// Per side, the palm in the slot's frame: skin centre, normal (out of the palm), along the fingers, toward the thumb.
__declspec(dllexport) volatile float CyberpunkVR_DebugGraspPalm[2][12] = {};
// Per side: 1 when the side the fingers close to and the hand's handedness disagree about the palm's normal.
__declspec(dllexport) volatile int CyberpunkVR_DebugGraspNormalFlip[2] = {};
// Per side, the thumb's search: its cost, the overlap (mm), the pad's gap to the thing (mm), the pad from its goal (mm).
__declspec(dllexport) volatile float CyberpunkVR_DebugGraspThumb[2][4] = {};
// Per side, the last Approach on a long thing: the palm's normal against its axis (|cos|), the palm along it from its middle
// (share of the half length, signed), the end the palm is at against the world's up (cos; 9 unknown), the kind it gave.
__declspec(dllexport) volatile float CyberpunkVR_DebugGraspApproach[2][4] = {};
// Fingers closed again capped by the finger beside them (the hand's coupling), since load.
__declspec(dllexport) volatile uint64_t CyberpunkVR_DebugGraspCoupled = 0;
}

namespace {
using cvr::anim::GraspLathe;

enum Shape : int { kBox = 0, kSphere = 1, kCylinder = 2, kCone = 3, kCapsule = 4 };
enum Kind : int { kFlat = 0, kWrap = 1, kBall = 2, kPinch = 3, kEdge = 4, kTop = 5, kCradle = 6, kTripod = 7 };
constexpr int kSlots = 19;
constexpr float kPi = 3.14159265f;
constexpr float kDeg = kPi / 180.0f;
constexpr float kPalmSkin = 0.014f;    // the palm's skin over the plane of the hand's bones (m)
constexpr float kPinchSize = 0.012f;   // a box no half size of which reaches this is pinched (m)
constexpr float kPinchBall = 0.012f;   // a ball under this radius is pinched (m)
constexpr float kTripodLong = 0.045f;  // a thing no half size of which passes this...
constexpr float kTripodMid = 0.022f;   // ...and whose middle half size (a ball's or a rod's radius) is under this: three fingers (m)
constexpr float kSidePinch = 0.020f;   // a plate this thick (half) and up to...
constexpr float kSidePinchMax = 0.030f; // ...this: taken by its side in three fingers (m)
constexpr float kTriInset = 0.012f;    // three fingers' pads this far in from a bigger thing's edge (m)...
constexpr float kTriSide = 0.012f;     // ...and its end on the little finger's side this far past them: over the middle finger
constexpr float kEndOn = 0.70f;        // a long thing taken by an end: the palm's normal this near its axis (cosine)...
constexpr float kEndNear = 0.55f;      // ...and the palm at least this share of its half length along it from its middle
constexpr float kEndDown = 0.35f;      // that end lower than this (cosine with the world's down): caught from below
constexpr float kTopGap = 0.014f;      // taken from above: its end this far under the palm's skin (m)
constexpr float kTopAhead = 0.012f;    // ...and its middle this far toward the fingers from the palm's (m)
constexpr float kCradleRise = 0.002f;  // caught from below: its end this far off the palm's skin (m)
constexpr float kPinchPad = 0.0085f;   // the index finger's pad over its bone (m)
constexpr float kWrapThick = 0.042f;   // a box is a bar when its two smaller half sizes are within this (m)
constexpr float kWrapRadius = 0.05f;   // a cylinder or a cone up to this radius is wrapped (m)
constexpr float kWrapBack = 0.008f;     // a wrapped thing's axis behind the knuckles (m)
constexpr float kWrapTilt = 14.0f * kDeg;
// Thick limbs lie more diagonally across the hand, but their axis stays behind the knuckles.
// Advancing it by 30 mm flattened the fingers against its underside instead of letting them wrap.
constexpr float kPartThin = 0.04f, kPartThick = 0.07f;
constexpr float kPartTiltThick = 32.0f * kDeg;
constexpr float kWrapOverhang = 0.055f; // a wrapped thing's thumb-side end at least this far past the index knuckle (m)
constexpr float kFlatMargin = 0.025f;  // the palm's centre kept this far inside a held face's edge (m)
// A face or a ball lies on the palm's pads (the thenar, the hypothenar) this far over its middle's skin (m).
constexpr float kPadRise = 0.006f;
// a face the palm lies on this far over the palm's plane: on the palm's pads (the real palm stands 5-9 mm over the plane
// at its middle, 20-30 mm at the thenar), not through them
constexpr float kFlatRise = 0.011f;
constexpr float kPlate = 0.45f;         // a box is a plate when its thinnest half size is under this share of the next
// A held plate pinched like a crab's claw: its near cover kEdgeGap past the knuckles' joints (under 0: behind them), its
// edge kEdgeLift off the palm -- the fingers curled onto one cover (the knuckle a little, the middle joint the most), the
// thumb bent onto the other, the plate between the pads. With its edge pressed into the palm and the cover 15 mm behind
// the knuckles the thumb's base stood 18 mm behind the other cover: room only to lie straight along it.
constexpr float kEdgeGap = -0.008f;
constexpr float kEdgeLift = 0.012f;
constexpr float kFlatHook = 0.006f;     // a held face's far edge, the fingers hooked over it, this far past the index knuckle (m)
constexpr float kFlatReach = 0.075f;    // ...when the edge is no further than this past it (the fingers reach over it)
constexpr float kStep = 1.5f * kDeg;
constexpr float kFollow = 0.2f;
// How much more a finger's joint may be bent than the same joint of the finger beside it, degrees, per pair (index-middle,
// middle-ring, ring-little) and joint (MCP, PIP, DIP). The middle, ring and little fingers share the deep flexor (the end
// joints) and are tied by the tendons between them, the ring and the little finger the most: neither bends at its two end
// joints with the other straight. The knuckles have muscles of their own and the index finger its own flexor.
constexpr float kCoupleDeg[3][3] = {{50.0f, 35.0f, 30.0f}, {35.0f, 20.0f, 15.0f}, {30.0f, 10.0f, 8.0f}};
constexpr float kPersonIn = 0.12f, kPersonOut = 0.18f;   // a person's part: the grasp eased in, and out (s)
constexpr float kPersonTurnTau = 0.08f;                  // ...and from one solve to the next (s)
constexpr float kPersonMoved = 0.025f;                   // re-solved once the part is this far off its seat (m)...
constexpr float kPersonTurned = 20.0f * kDeg;            // ...or turned this far from it...
constexpr double kPersonEvery = 0.5;                     // ...for this long (s)
constexpr float kSeatTurnRound = 45.0f * kDeg;           // a part held round is turned at most this far onto its seat...
constexpr float kSeatTurnPalm = 30.0f * kDeg;            // ...a palm on one turned at most this far to lie flat on it
constexpr float kSeatPalmGap = 0.007f;                   // the palm's plane this far off a part it lies on: on its pads (m)
constexpr float kSeatMost = 0.06f;                       // a seat takes the hand this far at the most (m)        // a finger joint's pace while the joint before it still closes

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
    case kCapsule: {
        const float z = p[2] - std::clamp(p[2], -h[2], h[2]);
        return std::sqrt(p[0] * p[0] + p[1] * p[1] + z * z) - h[0];
    }
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

// The signed distance from p (in the part's frame) to a lathe: to its section in (distance from the axis, height), the
// profile and its mirror across the axis closed by the two end caps (the far side is never nearer than the near one).
float LatheSdf(const GraspLathe& L, const float* p) {
    const int n = L.knots;
    const float q = std::hypot(p[0], p[1]), z = p[2];
    float v[36][2];
    for (int i = 0; i < n; ++i) {
        v[i][0] = L.r[i];
        v[i][1] = L.z[i];
        v[2 * n - 1 - i][0] = -L.r[i];
        v[2 * n - 1 - i][1] = L.z[i];
    }
    const int m = 2 * n;
    float d = (q - v[0][0]) * (q - v[0][0]) + (z - v[0][1]) * (z - v[0][1]), s = 1.0f;
    for (int i = 0, j = m - 1; i < m; j = i, ++i) {
        const float e[2] = {v[j][0] - v[i][0], v[j][1] - v[i][1]}, w[2] = {q - v[i][0], z - v[i][1]};
        const float ee = e[0] * e[0] + e[1] * e[1];
        const float t = ee > 1e-12f ? std::clamp((w[0] * e[0] + w[1] * e[1]) / ee, 0.0f, 1.0f) : 0.0f;
        const float b[2] = {w[0] - e[0] * t, w[1] - e[1] * t};
        d = std::min(d, b[0] * b[0] + b[1] * b[1]);
        const bool c0 = z >= v[i][1], c1 = z < v[j][1], c2 = e[0] * w[1] > e[1] * w[0];
        if ((c0 && c1 && c2) || (!c0 && !c1 && !c2)) s = -s;
    }
    return s * std::sqrt(d);
}

// How far the thing reaches from its centre along u (a unit vector in the slot's frame), turned by r.
float Support(int shape, const float* h, const float* r, const float* u) {
    float l[3];
    Unrot(r, u, l);
    const float side = std::sqrt(std::max(0.0f, 1.0f - l[2] * l[2]));
    switch (shape) {
    case kSphere: return h[0];
    case kCapsule: return h[0] + h[2] * std::fabs(l[2]);
    case kCylinder: return h[0] * side + h[2] * std::fabs(l[2]);
    case kCone: return std::max(h[0], h[1]) * side + h[2] * std::fabs(l[2]);
    default: return h[0] * std::fabs(l[0]) + h[1] * std::fabs(l[1]) + h[2] * std::fabs(l[2]);
    }
}

// A plate too thick for a cover's grip (see the header), taken by its side in three fingers.
bool SidePinch(int shape, const float* h) {
    if (shape != kBox) return false;
    float s[3] = {h[0], h[1], h[2]};
    std::sort(s, s + 3);
    return s[0] < kPlate * s[1] && s[1] > kWrapThick && s[0] >= kSidePinch && s[0] <= kSidePinchMax;
}

// The grasp a thing of this shape and size takes; `axis` its long axis (a bar's, a can's: z).
Kind Classify(int shape, const float* h, int& axis, bool part = false) {
    axis = 2;
    if (shape == kSphere) return h[0] < kPinchBall ? kPinch : h[0] < kTripodMid ? kTripod : kBall;
    if (shape == kCylinder || shape == kCone || shape == kCapsule) {
        const float radius = std::max(h[0], h[1]);
        if (!part && radius < kTripodMid && h[2] <= kTripodLong) return kTripod;
        return radius <= kWrapRadius ? kWrap : kFlat;
    }
    int order[3] = {0, 1, 2};
    std::sort(order, order + 3, [&](int a, int b) { return h[a] < h[b]; });
    if (h[order[2]] < kPinchSize) return kPinch;
    // three fingers: `axis` its thinnest way, the one between the pads and the thumb
    if (h[order[2]] <= kTripodLong && h[order[1]] < kTripodMid) { axis = order[0]; return kTripod; }
    axis = order[2];
    if (h[order[0]] < kPlate * h[order[1]] && h[order[1]] > kWrapThick) {
        axis = order[0];
        return SidePinch(shape, h) ? kTripod : kEdge;
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
    float index[3], pinky[3];              // the index finger's knuckle, the little finger's
    float pinch[3], pinchN[3];             // the index pad bent for a pinch, and the way out of it
    float tri[3], triN[3];                 // between the index and middle pads bent so, and the way out of them
    float side[3], sideN[3];               // the same bent as a claw closes (kSideBend): a thick plate's side
    Pose slot;                             // the weapon slot in the hand's frame
};

// Each segment a capsule round its bone's axis moved `palm` toward the palm (the way its hinge bends it) with `radius`:
// the game's first-person hand (a0_000_pwa_fpp__full_r) measured round the rig's bones at its reference
// (tools/grasp_tests GRASP_FIT): the bones run near the back of the fingers -- the middle and end phalanges' skin 1-2 mm
// behind the bone and 11-13 mm in front of it -- so the old capsules round the bone (9.5, 8.5, 7.5 mm) let the pads 3-9 mm
// into what they closed on.
struct Finger { int slot[3]; float maxDeg[3]; float speed[3]; float radius[3]; float palm[3]; float spread; };
const Finger kFingers[4] = {
    {{6, 11, 15}, {88.0f, 105.0f, 80.0f}, {1.0f, 1.15f, 0.85f}, {0.0100f, 0.0065f, 0.0059f}, {0.0022f, 0.0046f, 0.0049f}, 1.0f},
    {{7, 12, 16}, {88.0f, 105.0f, 80.0f}, {1.0f, 1.15f, 0.85f}, {0.0110f, 0.0077f, 0.0062f}, {0.0030f, 0.0056f, 0.0049f}, 0.0f},
    {{8, 13, 17}, {88.0f, 105.0f, 80.0f}, {1.0f, 1.15f, 0.85f}, {0.0094f, 0.0072f, 0.0060f}, {0.0012f, 0.0033f, 0.0029f}, -1.0f},
    {{9, 14, 18}, {88.0f, 105.0f, 80.0f}, {1.0f, 1.15f, 0.85f}, {0.0078f, 0.0053f, 0.0051f}, {0.0020f, 0.0031f, 0.0028f}, -1.8f},
};
const int kThumbSlots[3] = {0, 5, 10};
// the thumb's (the metacarpal's is the thenar's: not kept out of things)
const float kThumbRadius[3] = {0.012f, 0.0104f, 0.0079f};
const float kThumbPalm[3] = {0.0f, 0.0030f, 0.0023f};
// The thumb's end phalanx as the real one (GRASP_FIT's profile of the game's hand): at each share `u` of its length from
// the joint, the skin off its bone toward the pad, to either side and toward the nail (m). The pad is thick near the
// joint and thins to the tip, which turns to the nail: an even capsule (kThumbRadius off kThumbPalm, 10.2 mm toward the
// pad) stopped the real skin 4-6 mm short of a face pressed near the tip.
struct TipSlice { float u, pad, side, back; };
constexpr TipSlice kThumbTip[] = {
    {0.05f, 0.0101f, 0.0091f, 0.0051f}, {0.25f, 0.0102f, 0.0088f, 0.0041f}, {0.45f, 0.0087f, 0.0089f, 0.0044f},
    {0.65f, 0.0075f, 0.0085f, 0.0048f}, {0.75f, 0.0060f, 0.0082f, 0.0056f}, {0.85f, 0.0040f, 0.0076f, 0.0056f},
    {0.95f, 0.0021f, 0.0062f, 0.0061f}};
// A slice's skin off the bone in the unit way `toward` (an ellipse round it each side of the pad's way `padN`).
float TipExtent(const TipSlice& s, const float* padN, const float* toward) {
    const float c = Dot(padN, toward);
    const float a = c >= 0.0f ? s.pad : s.back;
    return std::sqrt(a * a * c * c + s.side * s.side * std::max(0.0f, 1.0f - c * c));
}

// A segment's axis: the bone's from joint a to joint b moved `off` toward the palm, the way `hingeLocal` (in the bone's
// frame `q`) bends it.
void SegAxis(const float* a, const float* b, const float* q, const float* hingeLocal, float off, float* oa, float* ob) {
    float ax[3], hw[3], pd[3];
    Sub(b, a, ax);
    Unit(ax);
    Rot(q, hingeLocal, hw);
    Cross(hw, ax, pd);
    Unit(pd);
    Mad(a, pd, off, oa);
    Mad(b, pd, off, ob);
}
constexpr float kPinchBend[3] = {32.0f * kDeg, 38.0f * kDeg, 16.0f * kDeg};
// A thick plate's side in three fingers, a crab's claw: the index and middle fingers closed already curled (kSideCurl)
// and led by the knuckle from there, met by the plate at its pads so bent (kSideBend: the curl and the knuckle's lead with
// the joints after it following at kFollow).
constexpr float kSideCurl[3] = {0.0f, 25.0f * kDeg, 10.0f * kDeg};
constexpr float kSideBend[3] = {28.0f * kDeg, 31.0f * kDeg, 15.0f * kDeg};
// ...the ring and little finger tucked under it, closed already curled into the fist
constexpr float kTuckCurl[3] = {0.0f, 60.0f * kDeg, 40.0f * kDeg};

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
    std::copy_n(p1.p, 3, pm.pinky);
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
    // The pinch: the index finger bent a little, its pad -- the distal phalanx's middle on the side it bends to. The
    // middle finger's the same way, for three fingers.
    auto pad = [&](const int* slots, const float* bend, float* at, float* out) {
        ChainBones c;
        float J[4][3], hinge[3][3], rot[3][4];
        Pose F[3];
        if (!ChainOf(buf, side, hand, slots, ref, c)) return false;
        for (int j = 0; j < 3; ++j) std::copy_n(ref[slots[j]], 4, rot[j]);
        Joints(c, rot, J, F);
        if (!Hinges(J, F, pm.n, hinge)) return false;
        for (int j = 0; j < 3; ++j) {
            float turn[4];
            AxisAngle(hinge[j], bend[j], turn);
            VRIK_QuatMul(ref[slots[j]], turn, rot[j]);
            VRIK_QuatNorm(rot[j]);
        }
        Joints(c, rot, J, F);
        float seg[3], axis[3];
        Sub(J[3], J[2], seg);
        Rot(F[2].q, hinge[2], axis);
        Cross(axis, seg, out);
        if (!Unit(out)) return false;
        for (int k = 0; k < 3; ++k) at[k] = J[2][k] + 0.55f * seg[k];
        Mad(at, out, kPinchPad, at);
        return true;
    };
    float middle[3], middleN[3];
    if (!pad(kFingers[0].slot, kPinchBend, pm.pinch, pm.pinchN)) return false;
    if (!pad(kFingers[1].slot, kPinchBend, middle, middleN)) return false;
    for (int k = 0; k < 3; ++k) {
        pm.tri[k] = 0.5f * (pm.pinch[k] + middle[k]);
        pm.triN[k] = pm.pinchN[k] + middleN[k];
    }
    if (!Unit(pm.triN)) return false;
    float si[3], siN[3], sm[3], smN[3];
    if (!pad(kFingers[0].slot, kSideBend, si, siN) || !pad(kFingers[1].slot, kSideBend, sm, smN)) return false;
    for (int k = 0; k < 3; ++k) {
        pm.side[k] = 0.5f * (si[k] + sm[k]);
        pm.sideN[k] = siN[k] + smN[k];
    }
    if (!Unit(pm.sideN)) return false;
    return pm.halfD > 0.01f;
}

// ---------------------------------------------------------------- how it is taken

// A long thing's axis and half length, in the hand's frame, for a thing at `pose` there.
float LongAxis(int shape, const float* h, int axis, const Pose& pose, float* a) {
    const float e[3] = {axis == 0 ? 1.0f : 0.0f, axis == 1 ? 1.0f : 0.0f, axis == 2 ? 1.0f : 0.0f};
    Rot(pose.q, e, a);
    return shape == kBox ? h[axis] : h[2];
}

// The grasp the hand takes a thing by, from where the hand is on it (see the header): its shape's, but a long thing
// taken by an end is TOP or CRADLE. `held` is the thing in the hand's frame, `up` the world's up there (null: unknown,
// so not CRADLE).
// `ends`: which of its ends may be taken so -- bit 0 its -Z end from above, 1 its +Z end, 2 its -Z end from below, 3 +Z.
Kind Approach(int shape, const float* h, const Pose& held, const Palm& pm, const float* up, int& axis,
              volatile float* note = nullptr, int ends = 15, bool part = false) {
    const Kind kind = Classify(shape, h, axis, part);
    if (kind != kWrap) return kind;
    float a[3], rel[3], end[3];
    const float length = LongAxis(shape, h, axis, held, a);
    Sub(pm.skin, held.p, rel);
    const float along = Dot(rel, a);
    for (int k = 0; k < 3; ++k) end[k] = along >= 0.0f ? a[k] : -a[k];
    const float endUp = up ? Dot(end, up) : 9.0f;
    Kind out = kWrap;
    if (std::fabs(Dot(pm.n, a)) >= kEndOn && std::fabs(along) >= kEndNear * length && Dot(pm.n, end) <= 0.0f) {
        out = up && endUp < -kEndDown ? kCradle : kTop;
        // that end of the thing's own Z (the axis's sign against it): one it may not be taken by is wrapped instead
        float z[3];
        const float e[3] = {0.0f, 0.0f, 1.0f};
        Rot(held.q, e, z);
        const bool plus = Dot(end, z) >= 0.0f;
        const int bit = (out == kCradle ? 4 : 1) << (plus ? 1 : 0);
        if (shape == kCylinder && !(ends & bit)) out = kWrap;
    }
    if (note) {
        note[0] = std::fabs(Dot(pm.n, a));
        note[1] = length > 0.0f ? along / length : 0.0f;
        note[2] = endUp;
        note[3] = static_cast<float>(out);
    }
    return out;
}

// The grasp of a thing already seated (the solve): its shape's; a long thing standing on the palm along its normal is
// TOP when its end is off the skin as the seat leaves it, CRADLE when it is on it.
Kind Seated(int shape, const float* h, const Pose& th, const Palm& pm, int& axis, bool part = false) {
    const Kind kind = Classify(shape, h, axis, part);
    if (kind != kWrap) return kind;
    float a[3], rel[3];
    const float length = LongAxis(shape, h, axis, th, a);
    const float c = std::fabs(Dot(pm.n, a));
    if (c < kEndOn) return kWrap;
    Sub(th.p, pm.skin, rel);
    const float gap = Dot(rel, pm.n) - length * c;
    return gap > 0.5f * (kTopGap + kCradleRise) ? kTop : kCradle;
}

// The world's up (the model's +Z) in each hand's frame, from the hand as the last pose pass left it (GraspFingers): the
// pass's own bones, so it holds in the lobby too, where no whole-skeleton snapshot is taken.
std::mutex g_upLock;
float g_up[2][3]{};
bool g_upKnown[2]{};
bool UpInHand(int side, float* up) {
    std::lock_guard<std::mutex> lk(g_upLock);
    if (side < 0 || side > 1 || !g_upKnown[side]) return false;
    std::copy_n(g_up[side], 3, up);
    return true;
}
// A bone's turn in the model's frame: its parents' local turns, the root's first.
bool ModelTurn(const uint8_t* buf, int bone, float* out) {
    int chain[64];
    int n = 0;
    for (int b = bone; b >= 0; b = g_VRBoneParent[b]) {
        if (!ValidBone(b) || n >= 64) return false;
        chain[n++] = b;
    }
    float acc[4] = {0.0f, 0.0f, 0.0f, 1.0f};
    for (int i = n - 1; i >= 0; --i) {
        const Pose l = Local(buf, chain[i]);
        float m[4];
        VRIK_QuatMul(acc, l.q, m);
        VRIK_QuatNorm(m);
        std::copy_n(m, 4, acc);
    }
    std::copy_n(acc, 4, out);
    return std::isfinite(acc[0] + acc[1] + acc[2] + acc[3]);
}
void NoteUp(const uint8_t* buf, int side, int hand) {
    float q[4], up[3];
    if (!ModelTurn(buf, hand, q)) return;
    const float z[3] = {0.0f, 0.0f, 1.0f};
    Unrot(q, z, up);
    std::lock_guard<std::mutex> lk(g_upLock);
    std::copy_n(up, 3, g_up[side]);
    g_upKnown[side] = true;
}

// ---------------------------------------------------------------- the state

// `force`: the grasp's kind as given (a person's part), not the shape's; `limit` the share of each joint's range the
// fingers may close (of the kind's own); `spread` the index finger's spread (radians, the others by their share) when
// given (>= 0), not the kind's.
struct Request {
    int shape = kBox; float half[3]{}; Pose obj; int ends = 15; bool part = false; GraspLathe lathe;
    int force = -1; float limit = 1.0f; float spread = -1.0f;
};

struct Side {
    std::mutex lock;
    bool active = false;
    bool pin = true;      // the slot held at the rig's reference while the hand holds (VRGraspPin)
    unsigned rev = 0, solved = ~0u;
    Request req;
    float blend = 0.0f;
    float rot[kSlots][4]{};
    bool have[kSlots]{};
    bool palm = false;
    Palm pm{};
    // A PERSON'S PART (GraspPerson): eased in and out here, not by the caller; re-solved as the part moves in the hand,
    // the fingers drawn eased from the last solve to the next (`shown`), so they never jump between two.
    bool person = false, personOn = false, shownOn = false;
    int style = 0;
    double easedAt = 0.0, solvedAt = 0.0;
    float shown[kSlots][4]{};
    unsigned taken = 0, resolved = 0;
};
Side g_side[2];

// The thing in the hand's frame, ready for distances: the part the palm is put to, or the whole of it round that part
// when it is known (a lathe: what the fingers rest on).
struct Thing {
    int shape;
    float half[3];
    Pose pose;
    GraspLathe lathe;
    float Dist(const float* p) const {
        float l[3];
        ToFrame(pose, p, l);
        return lathe.knots >= 2 ? LatheSdf(lathe, l) : Sdf(shape, half, l);
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
struct Closed { float J[4][3]; float A[3][3], B[3][3]; float radius[3]; Pose proximal; bool ok = false; };   // A, B: each segment's axis

// One finger closed: its three bones' rotations out of `start`, written to `out`; its angles (degrees) to `deg`.
int CloseFinger(const uint8_t* buf, int side, int hand, const Finger& f, const Thing& th, const Palm& pm,
                const float (*start)[4], float limit, float spread, float (*out)[4], float* deg, Closed& closed,
                const float* cap = nullptr, const float* spreadAbout = nullptr, const float* curl = nullptr) {
    ChainBones c;
    if (!ChainOf(buf, side, hand, f.slot, start, c)) return 0;
    float rot0[3][4], rot[3][4], J[4][3];
    Pose F[3];
    for (int j = 0; j < 3; ++j) std::copy_n(c.local[j].q, 4, rot0[j]);
    Joints(c, rot0, J, F);
    // Spread: the proximal phalanx turned in the palm's plane, about the bone axis nearest the palm's normal.
    // ...or (spreadAbout, the hand's frame) about that way at the knuckle, after the bend: a plate's cover's normal
    float pre[4] = {0.0f, 0.0f, 0.0f, 1.0f};
    if (spread != 0.0f && spreadAbout) {
        float turn[4], baseInv[4], m[4];
        AxisAngle(spreadAbout, spread, turn);
        VRIK_QuatConj(c.base.q, baseInv);
        VRIK_QuatMul(turn, c.base.q, m);
        VRIK_QuatMul(baseInv, m, pre);
        VRIK_QuatNorm(pre);
    } else if (spread != 0.0f) {
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
    for (int j = 0; j < 3; ++j) maxRad[j] = std::min(f.maxDeg[j] * kDeg * limit, cap ? std::max(0.0f, cap[j]) : 1e9f);
    auto pose = [&](const float* t) {
        for (int j = 0; j < 3; ++j) {
            float turn[4];
            AxisAngle(hinge[j], t[j], turn);
            VRIK_QuatMul(rot0[j], turn, rot[j]);
            if (j == 0) {
                float m[4];
                VRIK_QuatMul(pre, rot[0], m);
                std::copy_n(m, 4, rot[0]);
            }
            VRIK_QuatNorm(rot[j]);
        }
    };
    // A segment's clearance: from the thing, and (but the first segment) from the palm -- along its axis, off the bone.
    auto clearance = [&](int s, float* obj, float* palm) {
        *obj = *palm = 1e9f;
        float a[3], b[3];
        SegAxis(J[s], J[s + 1], F[s].q, hinge[s], f.palm[s], a, b);
        // the end phalanx over its pad only: the real fingertip narrows past it, turning to the nail
        for (float u : {0.25f, 0.5f, 0.75f, s == 2 ? 0.85f : 1.0f}) {
            float p[3];
            for (int k = 0; k < 3; ++k) p[k] = a[k] + (b[k] - a[k]) * u;
            *obj = std::min(*obj, th.Dist(p) - f.radius[s]);
            if (s > 0) *palm = std::min(*palm, PalmDist(pm, p) - f.radius[s]);
        }
    };
    // closed already curled (a claw's): from there on as any
    if (curl) {
        for (int j = 0; j < 3; ++j) theta[j] = std::min(std::max(0.0f, curl[j]), maxRad[j]);
        pose(theta);
        Joints(c, rot, J, F);
        if (th.shape == kCapsule) {
            // A moving limb can already occupy the pre-curl. Never start deeper than the open chain.
            bool clear = true;
            for (int s = 0; s < 3; ++s) {
                float o, p;
                clearance(s, &o, &p);
                clear &= o >= 0.0f && p >= 0.0f;
            }
            if (!clear) {
                for (float& t : theta) t = 0.0f;
                pose(theta);
                Joints(c, rot, J, F);
            }
        }
    }
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
        SegAxis(J[j], J[j + 1], F[j].q, hinge[j], f.palm[j], closed.A[j], closed.B[j]);
    }
    std::copy_n(&J[0][0], 12, &closed.J[0][0]);
    closed.proximal = F[0];
    closed.ok = true;
    return contacts;
}

// The thumb: its metacarpal turned to a way given by its own anatomical angles -- its elevation off the palm (palmar
// abduction) and its angle out from the index finger in the palm's plane (radial abduction; under 0 across the palm, toward
// the little finger) -- by the least turn from the rig's way, then rolled about itself (pronation: the pad turns to the
// fingers), and its two joints bent about their hinges. Searched -- a coarse grid, then finer steps one angle at a time --
// for the least cost: the thumb out of the thing, the palm and the closed fingers; its pad on the thing's surface, near the
// grasp's goal and facing it; the thumb near its ease for the grasp and within a real thumb's range.
// (Turned in front of the palm about the fingers' way and then about the palm's normal, as before 2026-11-10, the
// metacarpal could not rise past 49 deg off the palm, and every rise took it across the palm: the search, kept clear of
// the thing, swung it out sideways -- the thumb an L with the index finger beside a can's end.)
struct ThumbAngles { float elev, away, roll, mcp, ip; };

// THE THUMB AT EASE in each grasp (the metacarpal's elevation and angle out, its two joints' bend), which the comfort term
// pulls toward, so that a goal out of reach leaves the thumb as a hand would hold it. The rig's relaxed thumb: elevation
// 23 deg, out 44 deg.
//  - FLAT: laid on the face the palm is on, in the palm's plane, some 40 deg out from the index finger, barely bent;
//  - WRAP: abducted in front of the palm round the thing, opposite the fingers;
//  - BALL: abducted, wide;
//  - EDGE: a crab's claw on the other cover, bent at both joints, its pad across from the fingers' pads;
//  - TOP, CRADLE: round the rim, up the side;
//  - PINCH, TRIPOD: opposed, its pad to the index pad.
struct ThumbRest { float elev, away, mcp, ip; };
ThumbRest RestFor(Kind kind) {
    switch (kind) {
    case kFlat: return {0.0f, 40.0f * kDeg, 10.0f * kDeg, 10.0f * kDeg};
    case kBall: return {40.0f * kDeg, 40.0f * kDeg, 15.0f * kDeg, 25.0f * kDeg};
    case kEdge: return {55.0f * kDeg, 15.0f * kDeg, 25.0f * kDeg, 35.0f * kDeg};   // a crab's claw: bent at both joints
    case kPinch:
    case kTripod: return {45.0f * kDeg, 15.0f * kDeg, 20.0f * kDeg, 20.0f * kDeg};
    case kTop:
    case kCradle: return {35.0f * kDeg, 40.0f * kDeg, 15.0f * kDeg, 20.0f * kDeg};
    default: return {45.0f * kDeg, 35.0f * kDeg, 20.0f * kDeg, 30.0f * kDeg};
    }
}
// The roll that goes with opposition (per radian of the rise off the rig's elevation and half the turn in across the palm)
// and its free play; the comfort term's weight (per radian squared); a real thumb's elevation and angle out, past which a
// soft wall rises (kThumbWall per radian squared), and the most the whole thumb (base to tip) may point out sideways.
constexpr float kRollPerLift = 0.45f;
constexpr float kRollFree = 15.0f * kDeg;
constexpr float kThumbComfort = 0.06f;
constexpr float kElevEase[2] = {-5.0f * kDeg, 70.0f * kDeg}, kAwayEase[2] = {-25.0f * kDeg, 65.0f * kDeg};
constexpr float kWholeAway = 75.0f * kDeg;
// The pad's goal at most this share of the relaxed thumb's reach (base to pad) from its base.
constexpr float kThumbReach = 0.85f;
constexpr float kThumbWall = 2.0f;
float Past(float v, const float* range) { return v < range[0] ? range[0] - v : v > range[1] ? v - range[1] : 0.0f; }

bool SolveThumb(const uint8_t* buf, int side, int hand, Kind kind, int axis, const Thing& th, const Palm& pm,
                const float (*start)[4], const Closed* fingers, float (*out)[4], float* deg, bool person = false) {
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
    // Sparse first-web/thenar skin from the FPP mesh at the reference pose (GRASP_WEB_FIT).
    // Coordinates are (n,d,t) from pm.skin, followed by effective weights on thumb meta, thumb MCP and index MCP.
    // The remaining weight stays with the palm. These skin points rotate with the bones; an inflated limb does not.
    struct WebPoint { float n, d, t, meta, mcp, index; };
    static constexpr WebPoint webPoints[] = {
        {0.02052f, 0.02392f, 0.04861f, 0.08696f, 0.82213f, 0.00000f},
        {0.01385f, 0.02632f, 0.04190f, 0.26587f, 0.37302f, 0.00992f},
        {0.02311f, 0.02056f, 0.04158f, 0.19920f, 0.60159f, 0.00000f},
        {0.03049f, 0.00586f, 0.04095f, 0.36166f, 0.27668f, 0.00000f},
        {0.01671f, -0.02245f, 0.02022f, 0.27668f, 0.00000f, 0.00000f},
        {0.01244f, -0.02478f, 0.02541f, 0.25198f, 0.00000f, 0.00000f},
        {0.01925f, -0.01762f, 0.01526f, 0.31151f, 0.00000f, 0.00000f},
        {0.02654f, 0.01418f, 0.03818f, 0.31818f, 0.36364f, 0.00000f},
        {0.01018f, 0.00862f, 0.01110f, 0.33665f, 0.00000f, 0.00000f},
        {0.01179f, 0.01494f, 0.02054f, 0.44024f, 0.00398f, 0.00000f},
        {0.02768f, -0.00129f, 0.04032f, 0.45276f, 0.09449f, 0.00000f},
        {0.02187f, -0.00769f, 0.03987f, 0.47441f, 0.02756f, 0.00000f},
        {0.02763f, -0.00125f, 0.03305f, 0.47835f, 0.03937f, 0.00000f},
        {0.02483f, 0.00760f, 0.03002f, 0.46063f, 0.07874f, 0.00000f},
        {0.02241f, -0.01375f, 0.02750f, 0.42292f, 0.00000f, 0.00000f},
        {0.01581f, 0.00745f, 0.01534f, 0.43452f, 0.00000f, 0.00000f},
        {0.01762f, -0.00520f, 0.04653f, 0.44664f, 0.10277f, 0.00000f},
        {0.02637f, -0.00026f, 0.02611f, 0.48622f, 0.00787f, 0.00000f},
        {0.01728f, 0.01625f, 0.02990f, 0.44048f, 0.11111f, 0.00000f},
        {0.01425f, 0.02342f, 0.03363f, 0.39243f, 0.16335f, 0.00797f},
        {0.02299f, -0.00766f, 0.01659f, 0.41667f, 0.00000f, 0.00000f},
        {0.01921f, -0.00605f, 0.01046f, 0.35714f, 0.00000f, 0.00000f}};
    struct WebLocal { float fixed[3], meta[3], mcp[3]; };
    WebLocal web[sizeof(webPoints) / sizeof(webPoints[0])]{};
    if (person) {
        Pose indexRef;
        InFrame(buf, side, VRIK_LadderFingerBone(side, 6), hand, start, indexRef);
        for (size_t i = 0; i < sizeof(webPoints) / sizeof(webPoints[0]); ++i) {
            const WebPoint& w = webPoints[i];
            float p[3], indexLocal[3], indexPos[3];
            for (int k = 0; k < 3; ++k) p[k] = pm.skin[k] + pm.n[k] * w.n + pm.d[k] * w.d + pm.t[k] * w.t;
            ToFrame(F[0], p, web[i].meta);
            ToFrame(F[1], p, web[i].mcp);
            ToFrame(indexRef, p, indexLocal);
            const Pose& posed = fingers[0].ok ? fingers[0].proximal : indexRef;
            Rot(posed.q, indexLocal, indexPos);
            for (int k = 0; k < 3; ++k)
                web[i].fixed[k] = p[k] * (1.0f - w.meta - w.mcp - w.index) + (indexPos[k] + posed.p[k]) * w.index;
        }
    }
    // The metacarpal's way in the rig and its angles; the roll's axis, mirrored with the hand (the right hand's d x t is
    // against its palm's normal), so that a positive roll pronates either thumb.
    float dxt[3];
    Cross(pm.d, pm.t, dxt);
    const bool front = Dot(dxt, pm.n) > 0.0f;
    float meta[3], rollAxis[3];
    Sub(J[1], J[0], meta);
    Unit(meta);
    for (int k = 0; k < 3; ++k) rollAxis[k] = front ? -meta[k] : meta[k];
    const float elev0 = std::asin(std::clamp(Dot(meta, pm.n), -1.0f, 1.0f));
    const float away0 = std::atan2(Dot(meta, pm.t), Dot(meta, pm.d));
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
        // over the front of it, a little toward the fingertips: the thumb crosses it to meet the fingers coming round (toward
        // the wrist it went down the thing's near side, pointing back at the wrist)
        for (int k = 0; k < 3; ++k) u[k] = pm.n[k] * 0.85f + pm.d[k] * 0.35f;
        Mad(u, a, -Dot(u, a), u);
        if (!Unit(u)) std::copy_n(pm.n, 3, u);
        Mad(q, u, 0.2f, p);
        th.Nearest(p, goal);
        break;
    }
    case kBall: {
        float p[3];
        if (th.shape == kSphere && th.half[0] >= kWrapRadius) {
            // too big to close round: the palm lies on it, the thumb beside the index knuckle as on a face
            Mad(pm.index, pm.t, 0.025f, p);
            Mad(p, pm.n, kPalmSkin, p);
        } else {
            float dir[3];
            for (int k = 0; k < 3; ++k) dir[k] = pm.t[k] * 0.75f + pm.n[k] * 0.6f;
            Unit(dir);
            Mad(th.pose.p, dir, 1.0f, p);
        }
        th.Nearest(p, goal);
        break;
    }
    case kPinch: {
        float p[3];
        Mad(th.pose.p, pm.pinchN, 0.2f, p);
        th.Nearest(p, goal);
        break;
    }
    case kTripod: {
        // Its other side across from the two pads.
        const bool sideways = SidePinch(th.shape, th.half);
        float p[3];
        Mad(sideways ? pm.side : pm.tri, sideways ? pm.sideN : pm.triN, 0.2f, p);
        th.Nearest(p, goal);
        break;
    }
    case kTop:
    case kCradle: {
        // On its side, the other way from the fingers: from above just under the rim, toward the wrist; caught from
        // below a third of the way up, on the thumb's side.
        float a[3], rel[3], u[3], p[3];
        const float length = LongAxis(th.shape, th.half, axis, th.pose, a);
        Sub(pm.skin, th.pose.p, rel);
        const float s = Dot(rel, a) >= 0.0f ? 1.0f : -1.0f;      // toward the end at the palm
        if (kind == kTop) {
            Mad(th.pose.p, a, s * std::max(0.0f, length - 0.015f), p);
            for (int k = 0; k < 3; ++k) u[k] = pm.t[k] * 0.6f - pm.d[k] * 0.8f;
        } else {
            Mad(th.pose.p, a, s * length / 3.0f, p);
            for (int k = 0; k < 3; ++k) u[k] = pm.t[k] * 0.9f - pm.d[k] * 0.3f;
        }
        Mad(u, a, -Dot(u, a), u);
        if (!Unit(u)) std::copy_n(pm.t, 3, u);
        Mad(p, u, 0.2f, p);
        th.Nearest(p, goal);
        break;
    }
    case kEdge: {
        // The other cover right behind the index and middle fingers' pads (where they press it: a crab's claw), or,
        // without them, 6.5 cm out of the palm by the index finger.
        float p[3];
        if (fingers[0].ok && fingers[1].ok) {
            for (int k = 0; k < 3; ++k) p[k] = 0.0f;
            for (int i = 0; i < 2; ++i)
                for (int k = 0; k < 3; ++k)
                    p[k] += 0.5f * (fingers[i].J[2][k] + 0.6f * (fingers[i].J[3][k] - fingers[i].J[2][k]));
            Mad(p, pm.d, -0.10f, p);
        } else {
            Mad(pm.index, pm.n, 0.065f, p);
            Mad(p, pm.d, -0.06f, p);
        }
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
    // the joints' hard ends: the elevation and the angle out a little past a real thumb's (the soft wall keeps it in
    // range unless nothing else is clear), the two joints never back more than a pressed pad gives
    const float lo[5] = {-20.0f * kDeg, -50.0f * kDeg, -60.0f * kDeg, 0.0f * kDeg, -5.0f * kDeg};
    const float hi[5] = {80.0f * kDeg, 85.0f * kDeg, 60.0f * kDeg, 60.0f * kDeg, 80.0f * kDeg};
    auto rollWith = [&](float e, float p) {
        return std::clamp(kRollPerLift * ((e - elev0) + 0.5f * (away0 - p)), lo[2], hi[2]);
    };
    // A goal past the thumb's reach (a big part's far side) drawn in along the way to it from the thumb's base and back
    // onto the surface: the near side the thumb can get to, not a pull toward where it cannot.
    {
        float seg[3], pad[3], v[3];
        Sub(J[3], J[2], seg);
        Mad(J[2], seg, 0.7f, pad);
        Sub(pad, J[0], v);
        const float reach = kThumbReach * Len(v);
        Sub(goal, J[0], v);
        const float dist = Len(v);
        if (dist > reach && dist > 1e-4f) {
            float p[3];
            Mad(J[0], v, reach / dist, p);
            th.Nearest(p, goal);
        }
    }
    if (person && kind == kWrap) {
        // A limb wider than the hand is a partial wrap: seek the reachable near side, not its far crown.
        float pad[3];
        for (int k = 0; k < 3; ++k) pad[k] = J[2][k] + (J[3][k] - J[2][k]) * 0.6f;
        th.Nearest(pad, goal);
    }
    const ThumbRest rest = RestFor(kind);
    // the thumb lying on what the palm lies on: a face, a ball too big to close round
    const bool lies = kind == kFlat || (kind == kBall && th.shape == kSphere && th.half[0] >= kWrapRadius);
    // the pad itself opposed to the fingers' pads across a plate (a crab's claw): its roll free, the pad to face it
    const bool opposes = kind == kEdge || (kind == kTripod && SidePinch(th.shape, th.half));
    float penOut = 0.0f, gapOut = 0.0f, goalOut = 0.0f;
    auto cost = [&](const ThumbAngles& a, float (*rot)[4]) {
        float want[3], qd[4], qr[4], m[4], m2[4];
        const float ce = std::cos(a.elev), se = std::sin(a.elev), ca = std::cos(a.away), sa = std::sin(a.away);
        for (int k = 0; k < 3; ++k) want[k] = pm.n[k] * se + (pm.d[k] * ca + pm.t[k] * sa) * ce;
        VRIK_QuatFromTo(meta, want, qd);
        AxisAngle(rollAxis, a.roll, qr);
        VRIK_QuatMul(qd, qr, m2);
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
        float pen = 0.0f, nearest = 1e9f;
        if (person) {
            for (size_t i = 0; i < sizeof(webPoints) / sizeof(webPoints[0]); ++i) {
                float a[3], b[3], p[3];
                Rot(G[0].q, web[i].meta, a);
                Rot(G[1].q, web[i].mcp, b);
                for (int k = 0; k < 3; ++k)
                    p[k] = web[i].fixed[k] + (a[k] + G[0].p[k]) * webPoints[i].meta + (b[k] + G[1].p[k]) * webPoints[i].mcp;
                const float o = 0.001f - th.Dist(p);
                if (o > 0.0f) pen += o * o;
            }
        }
        // the pad's way: the way the distal joint bends it
        float seg[3], axis[3], padN[3];
        Sub(P[3], P[2], seg);
        Rot(G[2].q, hinge[2], axis);
        Cross(axis, seg, padN);
        Unit(padN);
        float padGap = 1e9f;   // the pad's half (the slices to two thirds of the way) by the real profile
        for (int s = 0; s < 3; ++s) {
            // The metacarpal is in the thenar's flesh. Limb wraps check it and the web above;
            // exposed phalanges use offset capsules, with the real profile for the tip.
            if (s == 0) {
                if (person) {
                    // The thenar follows the metacarpal. Unlike a loose item, a limb extends through the web,
                    // so its flesh must be checked too, not only the two exposed phalanges.
                    for (float u : {0.25f, 0.5f, 0.75f, 1.0f}) {
                        float p[3];
                        for (int k = 0; k < 3; ++k) p[k] = P[0][k] + (P[1][k] - P[0][k]) * u;
                        const float o = kThumbRadius[0] - th.Dist(p);
                        if (o > 0.0f) pen += o * o;
                    }
                }
                continue;
            }
            float segA[3], segB[3];
            SegAxis(P[s], P[s + 1], G[s].q, hinge[s], kThumbPalm[s], segA, segB);
            if (s == 2) {
                // the way to the thing's surface once, from the phalanx's middle (the slices span 2.5 cm)
                float mid[3], g[3];
                for (int k = 0; k < 3; ++k) mid[k] = 0.5f * (P[2][k] + P[3][k]);
                th.Grad(mid, g);
                for (const TipSlice& sl : kThumbTip) {
                    float p[3], toward[3];
                    for (int k = 0; k < 3; ++k) p[k] = P[2][k] + (P[3][k] - P[2][k]) * sl.u;
                    const float dist = th.Dist(p);
                    for (int k = 0; k < 3; ++k) toward[k] = dist >= 0.0f ? -g[k] : g[k];
                    const float o = TipExtent(sl, padN, toward) - dist;
                    if (o > 0.0f) pen += o * o;
                    nearest = std::min(nearest, -o);
                    if (sl.u <= 0.66f) padGap = std::min(padGap, -o);
                    const float q = kThumbRadius[s] - PalmDist(pm, p);
                    if (q > 0.0f) pen += q * q;
                }
            }
            for (float u : {0.0f, 0.25f, 0.5f, 0.75f, 1.0f}) {
                if (s == 2 || (s == 1 && u < 0.25f)) continue;
                float p[3];
                for (int k = 0; k < 3; ++k) p[k] = segA[k] + (segB[k] - segA[k]) * u;
                const float o = kThumbRadius[s] - th.Dist(p);
                if (o > 0.0f) pen += o * o;
                nearest = std::min(nearest, -o);
                const float q = kThumbRadius[s] - PalmDist(pm, p);
                if (q > 0.0f) pen += q * q;
            }
            for (int i = 0; i < 4; ++i) {
                if (!fingers[i].ok) continue;
                for (int g = 0; g < 3; ++g) {
                    const float o = kThumbRadius[s] + fingers[i].radius[g] -
                                    SegSeg(segA, segB, fingers[i].A[g], fingers[i].B[g]);
                    if (o > 0.0f) pen += o * o;
                }
            }
        }
        // The pad facing the thing: the way the distal joint bends it, against the surface's normal; on the segment's axis
        // off the bone.
        float pad[3], g[3];
        Mad(P[2], seg, 0.7f, pad);
        Mad(pad, padN, kThumbPalm[2], pad);
        // on a face the palm lies on the thumb lies too: touching with any part of it (its pad looks at the index finger)
        const float gap = lies ? nearest : padGap;
        float off[3];
        Sub(pad, goal, off);
        th.Grad(pad, g);
        const float facing = 1.0f + Dot(padN, g);
        // comfort: off the thumb at ease for this grasp (the angle out, the one a thumb minds most, the more), the roll off
        // what the opposition brings past its free play; past a real thumb's range a wall, and on the whole thumb out
        // sideways past kWholeAway (an L with the index finger)
        float tv[3];
        Sub(P[3], P[0], tv);
        Unit(tv);
        const float wholeAway = std::atan2(Dot(tv, pm.t), Dot(tv, pm.d));
        const float de = a.elev - rest.elev, dp = a.away - rest.away, dm = a.mcp - rest.mcp, di = a.ip - rest.ip;
        // (opposed to the fingers' pads across a plate the roll is the pad's to choose: turned to the cover, against the
        // lift's coupling)
        const float dr = opposes ? 0.0f : std::max(0.0f, std::fabs(a.roll - rollWith(a.elev, a.away)) - kRollFree);
        const float pe = Past(a.elev, kElevEase), pp = Past(a.away, kAwayEase);
        const float pw = std::max(0.0f, wholeAway - kWholeAway);
        // lying on a face the thumb follows it from its base, not bent at its joints (a sharp MCP reads as broken)
        const float jw = lies ? 3.0f : 1.0f;
        const float comfort = kThumbComfort * (de * de + 1.5f * dp * dp + jw * (dm * dm + di * di)) + 0.5f * dr * dr +
                              kThumbWall * (pe * pe + pp * pp + pw * pw);
        penOut = std::sqrt(pen);
        gapOut = gap;
        goalOut = Len(off);
        return 1e5f * pen + 2e3f * std::max(gap, 0.0f) * std::max(gap, 0.0f) + 30.0f * Dot(off, off) +
               (lies ? 0.0f : opposes ? 0.4f : 0.01f) * facing + comfort;
    };
    auto get = [](const ThumbAngles& a, int i) -> float {
        const float v[5] = {a.elev, a.away, a.roll, a.mcp, a.ip};
        return v[i];
    };
    auto set = [](ThumbAngles& a, int i, float v) {
        float* f[5] = {&a.elev, &a.away, &a.roll, &a.mcp, &a.ip};
        *f[i] = v;
    };
    float rot[3][4];
    ThumbAngles best{elev0, away0, 0, 0, 0};
    float bestCost = cost(best, rot);
    for (float e = -15.0f * kDeg; e <= 75.0f * kDeg + 1e-4f; e += 15.0f * kDeg)
        for (float p = -45.0f * kDeg; p <= 75.0f * kDeg + 1e-4f; p += 15.0f * kDeg)
            for (float m = 0.0f; m <= hi[3] + 1e-4f; m += 20.0f * kDeg)
                for (float i = 0.0f; i <= hi[4] + 1e-4f; i += 20.0f * kDeg)
                    for (int rk = 0; rk < (opposes ? 3 : 1); ++rk) {
                        // opposed across a plate also turned to it: the coupling's roll, then -60 and -20 deg
                        const float roll = rk == 0 ? rollWith(e, p) : rk == 1 ? lo[2] : -20.0f * kDeg;
                        const ThumbAngles a{e, p, roll, m, i};
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
    deg[0] = best.elev / kDeg;
    deg[1] = best.away / kDeg;
    deg[2] = best.mcp / kDeg;
    deg[3] = best.ip / kDeg;
    return true;
}

// The fingers' spread for a ball: wider the bigger it is (radians, the index finger's; the others by their share).
// Taken from above the fingertips go round the rim, spread by its size; caught from below they go up its sides.
float SpreadFor(Kind kind, const float* half) {
    if (kind == kBall) return std::clamp(half[0] * 1.6f, 0.08f, 0.22f);
    if (kind == kFlat) return 0.05f;
    if (kind == kEdge) return 0.0f;    // a cover's fingers: Solve's kCoverSpread
    if (kind == kTop) return std::clamp(std::max(half[0], half[1]) * 2.4f, 0.05f, 0.16f);
    if (kind == kCradle) return 0.04f;
    return 0.0f;
}

// How far a finger may close (a share of its joints' range): three fingers leave the ring and little finger curled out
// of the way, a pinch the three after the index.
float LimitFor(Kind kind, int finger) {
    if (kind == kPinch && finger >= 1) return 0.6f;
    if (kind == kTripod && finger >= 2) return 0.75f;
    return 1.0f;
}

void Solve(const uint8_t* buf, int side, int hand, Side& S) {
    float start[kSlots][4];
    ReferenceFingers(buf, side, start, S.have);
    for (int s = 0; s < kSlots; ++s) std::copy_n(start[s], 4, S.rot[s]);
    int axis;
    Thing th{S.req.shape, {S.req.half[0], S.req.half[1], S.req.half[2]}, Compose(S.pm.slot, S.req.obj)};
    th.lathe = S.req.lathe;
    Kind kind;
    if (S.req.force >= 0) {
        kind = static_cast<Kind>(S.req.force);
        axis = 2;
    } else {
        kind = Seated(S.req.shape, S.req.half, th.pose, S.pm, axis, S.req.part);
    }
    // Spread toward the thumb (index) or away from it (ring, pinky), whichever way that turns about the normal.
    float nxd[3];
    Cross(S.pm.n, S.pm.d, nxd);
    const float toThumb = Dot(nxd, S.pm.t) >= 0.0f ? 1.0f : -1.0f;
    const float spread = S.req.spread >= 0.0f ? S.req.spread : SpreadFor(kind, S.req.half);
    int contacts = 0;
    Closed closed[4];
    float degs[4][3] = {}, outs[4][3][4] = {};
    int gots[4] = {};
    const auto limitOf = [&](int i) { return LimitFor(kind, i) * std::clamp(S.req.limit, 0.05f, 1.0f); };
    // a plate's fingers spread on its cover: about n x t (the palm's d either way, the cover's normal within the knuckles'
    // slant), signed so that with the spread's own sign (toThumb) the index finger goes toward the thumb
    float about[3];
    Cross(S.pm.n, S.pm.t, about);
    for (float& v : about) v *= toThumb;
    const float* spreadAbout = kind == kEdge ? about : nullptr;
    // on a cover each finger its own (radians): the rig's bent fingers run together unevenly (the index tip 12 mm in toward
    // the middle finger, the little finger's 22 mm toward the ring finger), so these leave the tips some 16 mm apart
    constexpr float kCoverSpread[4] = {0.125f, 0.0f, -0.025f, -0.165f};
    // a thick plate's side: the index and middle fingers a crab's claw, the ring and little finger tucked a little apart
    // from them (the ring finger runs in toward the middle finger as it curls)
    const bool claw = kind == kTripod && SidePinch(th.shape, th.half);
    constexpr float kTuckSpread[4] = {0.0f, 0.0f, -0.12f, -0.15f};
    const auto spreadOf = [&](int i) {
        return (kind == kEdge ? kCoverSpread[i] : claw ? kTuckSpread[i] : kFingers[i].spread * spread) * toThumb;
    };
    const auto curlOf = [&](int i) -> const float* {
        return claw ? (i < 2 ? kSideCurl : kTuckCurl) : S.person && kind == kWrap ? kSideCurl : nullptr;
    };
    for (int i = 0; i < 4; ++i) {
        const Finger& f = kFingers[i];
        gots[i] = CloseFinger(buf, side, hand, f, th, S.pm, start, limitOf(i), spreadOf(i), outs[i], degs[i], closed[i],
                              nullptr, spreadAbout, curlOf(i));
    }
    // THE FINGERS AS ONE HAND: no joint more than kCoupleDeg[pair][joint] more bent than the same joint of the finger
    // beside it. A finger that closed further is closed again only so far: opened back along the way it closed, so out of
    // what it lay on.
    for (int pass = 0; pass < 2; ++pass)
        for (int i = 0; i < 4; ++i) {
            if (!closed[i].ok) continue;
            float cap[3] = {1e9f, 1e9f, 1e9f};
            bool over = false;
            for (int k : {i - 1, i + 1}) {
                if (k < 0 || k > 3 || !closed[k].ok) continue;
                const float* most = kCoupleDeg[std::min(i, k)];
                for (int j = 0; j < 3; ++j) {
                    cap[j] = std::min(cap[j], (degs[k][j] + most[j]) * kDeg);
                    over |= degs[i][j] > degs[k][j] + most[j] + 0.5f;
                }
            }
            if (!over) continue;
            const Finger& f = kFingers[i];
            gots[i] = CloseFinger(buf, side, hand, f, th, S.pm, start, limitOf(i), spreadOf(i), outs[i], degs[i], closed[i],
                                  cap, spreadAbout, curlOf(i));
            ++CyberpunkVR_DebugGraspCoupled;
        }
    for (int i = 0; i < 4; ++i) {
        if (!closed[i].ok) continue;
        contacts += gots[i];
        for (int j = 0; j < 3; ++j) {
            std::copy_n(outs[i][j], 4, S.rot[kFingers[i].slot[j]]);
            CyberpunkVR_DebugGraspAngles[side][5 + i * 3 + j] = degs[i][j];
        }
    }
    float thumb[3][4], deg[4] = {};
    if (SolveThumb(buf, side, hand, kind, axis, th, S.pm, start, closed, thumb, deg, S.person && kind == kWrap)) {
        for (int j = 0; j < 3; ++j) std::copy_n(thumb[j], 4, S.rot[kThumbSlots[j]]);
        for (int j = 0; j < 4; ++j) CyberpunkVR_DebugGraspAngles[side][1 + j] = deg[j];
    }
    CyberpunkVR_DebugGraspAngles[side][0] = static_cast<float>(kind);
    CyberpunkVR_DebugGrasp[3] = static_cast<uint64_t>(contacts);
    ++CyberpunkVR_DebugGrasp[0];
}

// The seat of a thing in the slot's frame (see the header): `p`, `q` where it is in the slot when the hand takes it;
// `up` the world's up in the hand's frame (null: unknown). `kindOut`, if given, the grasp it is seated for.
bool Seat(int side, int shape, const float* half, const float* p, const float* q, const float* up, Pose& out,
          int* kindOut = nullptr, int ends = 15, bool part = false, const GraspLathe* lathe = nullptr) {
    Side& S = g_side[side];
    std::lock_guard<std::mutex> lk(S.lock);
    if (!S.palm) return false;
    const Palm& pm = S.pm;
    Pose item;
    std::copy_n(p, 3, item.p);
    std::copy_n(q, 4, item.q);
    VRIK_QuatNorm(item.q);
    const Pose held = Compose(pm.slot, item);   // in the hand's frame
    int axis;
    const Kind kind = Approach(shape, half, held, pm, up, axis, CyberpunkVR_DebugGraspApproach[side], ends, part);
    if (kindOut) *kindOut = kind;
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
        // The thin axis square to the knuckles' line in the palm's plane (the line runs back from the index knuckle to
        // the little finger's), the longest out of the palm (the short edge across it), the least turns; the cover
        // kEdgeGap behind the knuckles' joints, so that each finger folds onto it, the edge on the palm, its middle at
        // the knuckles' middle.
        float line[3], c[3];
        Sub(pm.index, pm.pinky, line);
        Mad(line, pm.n, -Dot(line, pm.n), line);
        if (!Unit(line)) std::copy_n(pm.t, 3, line);
        Cross(pm.n, line, c);
        if (Dot(c, pm.d) < 0.0f) for (float& v : c) v = -v;
        Unit(c);
        float a[3], to[3];
        axisOf(axis, a);
        for (int k = 0; k < 3; ++k) to[k] = Dot(a, c) >= 0.0f ? c[k] : -c[k];
        turnOnto(a, to);
        int longest = -1;
        for (int k = 0; k < 3; ++k)
            if (k != axis && (longest < 0 || half[k] > half[longest])) longest = k;
        alignAbout(longest, to, pm.n);
        float rel[3];
        Sub(pm.knuckle, pm.skin, rel);
        Mad(rel, pm.n, -Dot(rel, pm.n), rel);
        for (int k = 0; k < 3; ++k) pos[k] = pm.skin[k] + rel[k];
        Mad(pos, c, -(kEdgeGap + Support(shape, half, r, c)), pos);
        // its edge off the palm: held between the pads
        Mad(pos, pm.n, Support(shape, half, r, pm.n) + kEdgeLift, pos);
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
    case kTripod: {
        // Its thinnest way between the pads and the thumb, its long side across the two fingers; a rod across them.
        float a[3], to[3];
        if (shape == kBox) {
            const bool sideways = SidePinch(shape, half);
            const float* at = sideways ? pm.side : pm.tri;
            const float* atN = sideways ? pm.sideN : pm.triN;
            axisOf(axis, a);
            for (int k = 0; k < 3; ++k) to[k] = Dot(a, atN) >= 0.0f ? atN[k] : -atN[k];
            turnOnto(a, to);
            int longest = -1;
            for (int k = 0; k < 3; ++k)
                if (k != axis && (longest < 0 || half[k] > half[longest])) longest = k;
            alignAbout(longest, atN, pm.t);
            // A bigger thing held near its edge, the rest of it out of the palm and out to the thumb's side: the pads
            // kTriInset in from that edge, its end on the little finger's side over the middle finger, so the ring and
            // little finger curl free under it.
            const int away = 3 - axis - longest;
            Mad(at, atN, Support(shape, half, r, atN), pos);
            if (sideways) {
                float u[3], w[3], toIndex[3];
                axisOf(away, u);
                if (Dot(u, pm.n) < 0.0f) for (float& v : u) v = -v;
                Mad(pos, u, half[away] - kTriInset, pos);
                axisOf(longest, w);
                Sub(pm.index, pm.pinky, toIndex);
                if (Dot(w, toIndex) < 0.0f) for (float& v : w) v = -v;
                Mad(pos, w, std::max(0.0f, half[longest] - kTriSide), pos);
            }
            break;
        } else if (shape != kSphere) {
            axisOf(2, a);
            for (int k = 0; k < 3; ++k) to[k] = Dot(a, pm.t) >= 0.0f ? pm.t[k] : -pm.t[k];
            turnOnto(a, to);
        }
        Mad(pm.tri, pm.triN, Support(shape, half, r, pm.triN), pos);
        break;
    }
    case kTop:
    case kCradle: {
        // Its axis along the palm's normal, the end the hand came to toward the palm; a bar's faces along the fingers.
        float a[3], rel[3], end[3], to[3];
        axisOf(axis, a);
        Sub(pm.skin, held.p, rel);
        for (int k = 0; k < 3; ++k) {
            end[k] = Dot(rel, a) >= 0.0f ? a[k] : -a[k];
            to[k] = -pm.n[k];
        }
        turnOnto(end, to);
        if (shape == kBox) edgeOnto(axis, pm.n, pm.d, pm.t);
        const float length = shape == kBox ? half[axis] : half[2];
        // From above: the end a little under the palm, toward the fingers, for the fingertips to go round its rim.
        // From below: standing on the palm's middle.
        if (kind == kTop) Mad(pm.skin, pm.d, kTopAhead, pos);
        else std::copy_n(pm.skin, 3, pos);
        Mad(pos, pm.n, length + (kind == kTop ? kTopGap : kCradleRise), pos);
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
        // Its far edge where the fingers reach over it: held with that edge just past the knuckles, the fingers hooked
        // round it, their pads on the side face (with the edge under the middle joints only an end joint could bend
        // round it). Not if that took the face off the palm's centre.
        {
            float off[3];
            Sub(pos, pm.skin, off);
            const float sd = Support(shape, half, r, pm.d);
            float ki[3];
            Sub(pm.index, pm.skin, ki);
            const float knuckle = Dot(ki, pm.d), edge = Dot(off, pm.d) + sd;
            const float over = edge - knuckle;
            if (over > kFlatHook && over < kFlatReach) {
                const float shift = kFlatHook - over;
                if (Dot(off, pm.d) + shift + sd >= kFlatMargin && Dot(off, pm.d) + shift - sd <= -kFlatMargin)
                    Mad(pos, pm.d, shift, pos);
            }
        }
        Mad(pos, pm.n, Support(shape, half, r, pm.n) + kFlatRise, pos);
        break;
    }
    }
    // A part wrapped (a bottle's neck) has the whole thing round it: the palm put to the neck would be in the shoulder
    // where the hand is wider than the neck is long. Off the palm, along its normal, until the palm's skin and the
    // knuckles (a proximal bone's radius round them) are out of the whole -- a neck is held in the fingers, the palm off it.
    if (lathe && lathe->knots >= 2 && kind == kWrap) {
        for (int pass = 0; pass < 4; ++pass) {
            float worst = 0.0f;
            const auto at = [&](const float* P, float clear) {
                float rel[3], l[3];
                Sub(P, pos, rel);
                Unrot(r, rel, l);
                worst = std::min(worst, LatheSdf(*lathe, l) - clear);
            };
            for (int i = -2; i <= 2; ++i)
                for (int j = -1; j <= 1; ++j) {
                    float P[3];
                    Mad(pm.skin, pm.d, 0.45f * i * pm.halfD, P);
                    Mad(P, pm.t, 0.9f * j * pm.halfT, P);
                    at(P, 0.0f);
                }
            for (int j = -2; j <= 2; ++j) {
                float P[3];
                Mad(pm.knuckle, pm.t, 0.5f * j * pm.halfT, P);
                at(P, kFingers[0].radius[0]);
            }
            if (worst >= -0.0005f) break;
            Mad(pos, pm.n, -worst + 0.001f, pos);
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

namespace {
double NowS() {
    using namespace std::chrono;
    return duration<double>(steady_clock::now().time_since_epoch()).count();
}
}  // namespace

namespace cvr::anim {

void GraspFingers(uint8_t* buf) {
    if (!buf || g_VRBoneCount <= 0) return;
    for (int side = 0; side < 2; ++side) {
        const int hand = side ? g_VRRightBoneIdx : g_VRLeftBoneIdx;
        const int slotBone = side ? g_VRSmokeCigIdx : g_VRSmokeLighterIdx;
        if (!ValidBone(hand) || !ValidBone(slotBone)) continue;
        NoteUp(buf, side, hand);
        Side& S = g_side[side];
        bool holding;
        {
            std::lock_guard<std::mutex> lk(S.lock);
            holding = S.active && S.pin;
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
        if (S.person) {
            const double now = NowS();
            const float dt = S.easedAt > 0.0 ? static_cast<float>(std::clamp(now - S.easedAt, 0.0, 0.1)) : 0.0f;
            S.easedAt = now;
            S.blend = S.personOn ? std::min(1.0f, S.blend + dt / kPersonIn) : std::max(0.0f, S.blend - dt / kPersonOut);
            if (!S.personOn && S.blend <= 0.0f) {
                S.active = S.person = S.shownOn = false;
                continue;
            }
            // the fingers drawn eased toward the last solve (the first is taken as it is)
            const float k = S.shownOn ? 1.0f - std::exp(-dt / kPersonTurnTau) : 1.0f;
            for (int s = 0; s < kSlots; ++s) {
                if (!S.have[s]) continue;
                float* a = S.shown[s];
                const float* t = S.rot[s];
                if (k >= 0.999f) {
                    std::copy_n(t, 4, a);
                    continue;
                }
                const float w = a[0] * t[0] + a[1] * t[1] + a[2] * t[2] + a[3] * t[3] < 0.0f ? -k : k;
                for (int c = 0; c < 4; ++c) a[c] = a[c] * (1.0f - k) + t[c] * w;
                VRIK_QuatNorm(a);
            }
            S.shownOn = true;
        }
        const float b = S.blend;
        if (b <= 0.001f) continue;
        for (int s = 0; s < kSlots; ++s) {
            if (!S.have[s]) continue;
            const int bone = VRIK_LadderFingerBone(side, s);
            if (!ValidBone(bone)) continue;
            float* q = reinterpret_cast<float*>(buf + bone * 48 + VRIK_ROT_OFF);
            const float* t = S.person ? S.shown[s] : S.rot[s];
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

bool GraspTake(int side, int shape, const float* half, const float* p, const float* q, float* seatP, float* seatQ,
               int* kind, int ends, bool part, const GraspLathe* lathe) {
    if (side < 0 || side > 1 || shape < kBox || shape > kCone) return false;
    Side& S = g_side[side];
    Pose slot;
    {
        std::lock_guard<std::mutex> lk(S.lock);
        if (!S.palm) return false;
        slot = S.pm.slot;
    }
    // the hand's frame to the slot's, the seat, and back
    Pose inHand;
    std::copy_n(p, 3, inHand.p);
    std::copy_n(q, 4, inHand.q);
    VRIK_QuatNorm(inHand.q);
    Pose inSlot;
    float inv[4];
    ToFrame(slot, inHand.p, inSlot.p);
    VRIK_QuatConj(slot.q, inv);
    VRIK_QuatMul(inv, inHand.q, inSlot.q);
    VRIK_QuatNorm(inSlot.q);
    float up[3];
    const bool upKnown = UpInHand(side, up);
    Pose seat;
    int k = kFlat;
    if (!Seat(side, shape, half, inSlot.p, inSlot.q, upKnown ? up : nullptr, seat, &k, ends, part, lathe)) return false;
    const Pose back = Compose(slot, seat);
    std::copy_n(back.p, 3, seatP);
    std::copy_n(back.q, 4, seatQ);
    if (kind) *kind = k;
    std::lock_guard<std::mutex> lk(S.lock);
    S.req.shape = shape;
    std::copy_n(half, 3, S.req.half);
    S.req.obj = seat;
    S.req.ends = ends;
    S.req.part = part;
    S.req.lathe = lathe && lathe->knots >= 2 ? *lathe : GraspLathe{};
    // a thing's own grasp: none of a person's part's forced kind, limit or spread left from the last hold
    S.req.force = -1;
    S.req.limit = 1.0f;
    S.req.spread = -1.0f;
    S.active = true;
    S.pin = true;
    ++S.rev;
    return true;
}

bool GraspPalm(int side, float* skin) {
    if (side < 0 || side > 1) return false;
    Side& S = g_side[side];
    std::lock_guard<std::mutex> lk(S.lock);
    if (!S.palm) return false;
    std::copy_n(S.pm.skin, 3, skin);
    return true;
}

// THE PART A HAND CLOSES ON, of a round long thing given by the points of its hull (or of its drawn mesh's profile,
// tools/grasp_tests/profiles). The radius round the axis is read in sixteen slices along it (a hull's sides have no
// points between its rings, so an empty slice is the straight line between the slices round it); a NECK is a run of
// slices under 62% of the widest at one end, 2 cm long at least and no more than half the thing, while the other end's
// three slices keep over 85% -- a bottle, whose neck can be a fifth of it (a rum's). Its SHOULDER is where, coming from
// the neck's end, the radius first passes 85% of the widest. The neck is then a cylinder from its end to the shoulder, of the radius of its end's two slices
// (the mouth: the hull fills the neck's waist, its rim is still the neck's width), and the body a cylinder from the
// other end to the shoulder, of the widest radius. A hand past the middle of the shoulder's slope, toward the neck,
// closes on the neck; any other on the body.
bool GraspPartOf(const float* points, int n, float halfLength, const float* hand, GraspPart& out) {
    constexpr int kSlices = 16;
    if (!points || n < 6 || !(halfLength > 0.01f) || !hand) return false;
    float r[kSlices];
    for (float& v : r) v = -1.0f;
    const float w = 2.0f * halfLength / kSlices;
    for (int i = 0; i < n; ++i) {
        const float* p = points + 3 * i;
        const int k = std::clamp(static_cast<int>((p[2] + halfLength) / w), 0, kSlices - 1);
        r[k] = std::max(r[k], std::sqrt(p[0] * p[0] + p[1] * p[1]));
    }
    int first = -1, last = -1;
    for (int k = 0; k < kSlices; ++k)
        if (r[k] >= 0.0f) { if (first < 0) first = k; last = k; }
    if (first < 0) return false;
    for (int k = 0; k < first; ++k) r[k] = r[first];
    for (int k = last + 1; k < kSlices; ++k) r[k] = r[last];
    for (int k = first + 1; k < last; ++k) {
        if (r[k] >= 0.0f) continue;
        int b = k + 1;
        while (r[b] < 0.0f) ++b;
        const int a = k - 1;
        r[k] = r[a] + (r[b] - r[a]) * float(k - a) / float(b - a);
    }
    float widest = 0.0f;
    for (float v : r) widest = std::max(widest, v);
    if (!(widest > 0.005f)) return false;
    // the whole thing as a lathe: its ends, and each slice's radius at the slice's middle
    out.lathe.knots = kSlices + 2;
    out.lathe.z[0] = -halfLength;
    out.lathe.r[0] = r[0];
    for (int k = 0; k < kSlices; ++k) {
        out.lathe.z[k + 1] = -halfLength + w * (k + 0.5f);
        out.lathe.r[k + 1] = r[k];
    }
    out.lathe.z[kSlices + 1] = halfLength;
    out.lathe.r[kSlices + 1] = r[kSlices - 1];
    {
        // a knot on the straight line between the last one kept and the next (within half a millimetre) is dropped: a
        // bottle's body is one straight run, and the fingers' distances to the lathe are read thousands of times a take
        GraspLathe& L = out.lathe;
        int kept = 1;
        for (int k = 1; k + 1 < L.knots; ++k) {
            const float t = (L.z[k] - L.z[kept - 1]) / std::max(1e-6f, L.z[k + 1] - L.z[kept - 1]);
            const float line = L.r[kept - 1] + (L.r[k + 1] - L.r[kept - 1]) * t;
            if (std::fabs(L.r[k] - line) <= 0.0005f) continue;
            L.z[kept] = L.z[k];
            L.r[kept] = L.r[k];
            ++kept;
        }
        L.z[kept] = L.z[L.knots - 1];
        L.r[kept] = L.r[L.knots - 1];
        L.knots = kept + 1;
    }
    out.z = 0.0f;
    int s = 0, thin = 0;     // the neck's end: +1 the +Z end, -1 the -Z end; its thin run's slices
    for (int end = 1; end >= -1 && !s; end -= 2) {
        int run = 0;
        while (run < kSlices && r[end > 0 ? kSlices - 1 - run : run] < 0.62f * widest) ++run;
        const float other = std::max(r[end > 0 ? 0 : kSlices - 1], std::max(r[end > 0 ? 1 : kSlices - 2], r[end > 0 ? 2 : kSlices - 3]));
        if (run >= 1 && run * w >= 0.02f && run <= kSlices / 2 && other > 0.85f * widest) {s = end; thin = run;}
    }
    if (!s) return false;
    // from the neck's end: the shoulder, the first slice past 85% of the widest
    int shoulder = -1;
    for (int i = 0; i < kSlices; ++i) {
        const int k = s > 0 ? kSlices - 1 - i : i;
        if (r[k] >= 0.85f * widest) { shoulder = k; break; }
    }
    if (shoulder < 0) return false;
    const int mouth = s > 0 ? kSlices - 1 : 0, next = s > 0 ? kSlices - 2 : 1;
    const float neckR = std::max(0.008f, std::max(r[mouth], r[next]));
    // the shoulder's z (its slice's edge on the neck's side), and where its slope is half way down to the neck
    const float zShoulder = -halfLength + w * (s > 0 ? shoulder + 1 : shoulder);
    int slope = shoulder;
    for (int i = 0; i < kSlices; ++i) {
        const int k = shoulder + s * i;
        if (k < 0 || k >= kSlices) break;
        if (r[k] <= 0.5f * (widest + neckR)) { slope = k; break; }
    }
    const float zSlope = -halfLength + w * (slope + 0.5f);
    const float zEnd = s * halfLength, zBase = -s * halfLength;
    // the neck the palm is put to: its thin run from the mouth (the lathe has the shoulder beyond it, for the fingers)
    const float zThin = zEnd - s * w * thin;
    if (s * (hand[2] - zSlope) >= 0.0f) {
        out.z = 0.5f * (zEnd + zThin);
        out.half = 0.5f * std::fabs(zEnd - zThin);
        out.radius = neckR;
        out.ends = s > 0 ? 2 : 1;                       // over the mouth only; never stood on the neck
        out.neck = true;
    } else {
        out.z = 0.5f * (zBase + zShoulder);
        out.half = 0.5f * std::fabs(zShoulder - zBase);
        out.radius = widest;
        out.ends = s > 0 ? (1 | 4) : (2 | 8);            // its base: from above or stood on the palm; not its shoulder
        out.neck = false;
    }
    for (int k = 0; k < out.lathe.knots; ++k) out.lathe.z[k] -= out.z;     // into the part's frame
    return out.half > 0.005f;
}

void GraspEase(int side, float blend) {
    if (side < 0 || side > 1) return;
    Side& S = g_side[side];
    std::lock_guard<std::mutex> lk(S.lock);
    S.blend = std::clamp(blend, 0.0f, 1.0f);
}

void GraspLetGo(int side) {
    if (side < 0 || side > 1) return;
    Side& S = g_side[side];
    std::lock_guard<std::mutex> lk(S.lock);
    S.active = false;
    S.person = S.personOn = S.shownOn = false;
    S.blend = 0.0f;
}

// THE SEAT OF A PERSON'S PART (its capsule's pose in the hand bone's frame, the axis z, `half` its segment's half length):
//  - held round it (styles 1-3): as a can -- its axis turned (kSeatTurnRound at most) onto the palm's width tilted toward
//    the fingers, through a point a little behind the knuckles, its surface on the palm's skin; its place along its axis
//    kept. Not when its axis points into the palm (an end of it): false, the palm instead;
//  - the palm (4): the hand turned (kSeatTurnPalm at most) for the palm's normal to point at the part's axis, its skin
//    kSeatPalmGap off the part's surface.
static float Dot4(const float* a, const float* b) { return a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]; }

static bool PersonSeat(const Palm& pm, int style, const Pose& cap, float radius, float half, Pose& out) {
    out = cap;
    const float ez[3] = {0.0f, 0.0f, 1.0f};
    float a[3];
    Rot(cap.q, ez, a);
    if (style != 4) {
        if (std::fabs(Dot(a, pm.n)) > 0.6f) return false;
        const float thick = std::clamp((radius - kPartThin) / (kPartThick - kPartThin), 0.0f, 1.0f);
        const float tilt = kWrapTilt + (kPartTiltThick - kWrapTilt) * thick;
        float w[3];
        for (int k = 0; k < 3; ++k) w[k] = pm.t[k] * std::cos(tilt) + pm.d[k] * std::sin(tilt);
        Unit(w);
        if (Dot(a, w) < 0.0f)
            for (float& v : w) v = -v;
        float about[3];
        Cross(a, w, about);
        const float angle = std::acos(std::clamp(Dot(a, w), -1.0f, 1.0f));
        if (Unit(about) && angle > 1e-3f) {
            float turn[4];
            AxisAngle(about, std::min(angle, kSeatTurnRound), turn);
            VRIK_QuatMul(turn, cap.q, out.q);
            VRIK_QuatNorm(out.q);
        }
        float a2[3], c0[3], rel[3];
        Rot(out.q, ez, a2);
        Mad(pm.skin, pm.d, pm.halfD - kWrapBack, c0);
        // Seat on the real palm pads, not the bone-plane skin estimate through the thenar/web.
        Mad(c0, pm.n, radius + kFlatRise, c0);
        Sub(cap.p, c0, rel);
        Mad(c0, a2, Dot(rel, a2), out.p);
        return true;
    }
    // the way from the palm's skin to the part's axis
    float rel[3], onAxis[3], v[3];
    Sub(pm.skin, cap.p, rel);
    Mad(cap.p, a, std::clamp(Dot(rel, a), -half, half), onAxis);
    Sub(onAxis, pm.skin, v);
    if (!Unit(v)) return false;
    // the hand turned about the skin by r (n onto v): the part, in the turned hand, turned back by it about the skin
    float about[3], r[4] = {0.0f, 0.0f, 0.0f, 1.0f}, ri[4];
    Cross(pm.n, v, about);
    const float angle = std::acos(std::clamp(Dot(pm.n, v), -1.0f, 1.0f));
    if (Unit(about) && angle > 1e-3f) AxisAngle(about, std::min(angle, kSeatTurnPalm), r);
    VRIK_QuatConj(r, ri);
    float off[3], back[3];
    Sub(cap.p, pm.skin, off);
    Rot(ri, off, back);
    for (int k = 0; k < 3; ++k) out.p[k] = pm.skin[k] + back[k];
    VRIK_QuatMul(ri, cap.q, out.q);
    VRIK_QuatNorm(out.q);
    // then along the palm's normal, the skin kSeatPalmGap off its surface
    float a2[3];
    Rot(out.q, ez, a2);
    Sub(pm.skin, out.p, rel);
    Mad(out.p, a2, std::clamp(Dot(rel, a2), -half, half), onAxis);
    Sub(onAxis, pm.skin, v);
    Mad(out.p, pm.n, radius + kSeatPalmGap - Dot(v, pm.n), out.p);
    return true;
}

// ON THE PALM ALONG ITS NORMAL (GraspPerson's palmGap): the part moved across its own axis only, for the point of its axis
// nearest the palm's skin to lie `radius + gap` out along the palm's normal from that skin. Its way and its place along it
// are kept, and there is no limit to how far: the caller placed it by an estimate of the palm, this is the palm itself.
static bool PersonOnPalm(const Palm& pm, const Pose& cap, float radius, float half, float gap, Pose& out) {
    out = cap;
    const float ez[3] = {0.0f, 0.0f, 1.0f};
    float a[3], rel[3], onAxis[3], want[3], move[3];
    Rot(cap.q, ez, a);
    Sub(pm.skin, cap.p, rel);
    Mad(cap.p, a, std::clamp(Dot(rel, a), -half, half), onAxis);
    Mad(pm.skin, pm.n, radius + gap, want);
    Sub(want, onAxis, move);
    const float along = Dot(move, a);
    Mad(move, a, -along, move);
    for (int k = 0; k < 3; ++k) out.p[k] = cap.p[k] + move[k];
    return std::isfinite(out.p[0] + out.p[1] + out.p[2]);
}

// A PERSON'S PART IN A PHYSICS HAND (see the header). Each style a grasp:
//   1 a can: WRAP round the capsule, every joint as far as it goes (a forearm, an upper arm, a shoulder's edge, a thigh,
//     a hip's side);
//   2 a can with the fingers spread wide (a neck: the hand round its side, the fingers over the nape);
//   3 gently: WRAP, the joints to 60 % of their range and a little spread (a hand, its fingers);
//   4 the palm: FLAT, the fingers lying on it, closing no further than 60 % of their range (a face, a head, a chest, a
//     back, the buttocks).
bool GraspPerson(int side, int style, const float* p, const float* q, float halfLength, float radius, float* seatP,
                 float* seatQ, bool* seated, float palmGap) {
    if (seated) *seated = false;
    const int asked = style;
    if (side < 0 || side > 1) return false;
    Side& S = g_side[side];
    std::unique_lock<std::mutex> lk(S.lock, std::try_to_lock);
    if (!lk.owns_lock()) return false;   // the pose pass solving: the next step
    if (style <= 0 || !p || !q) {
        if (S.person && S.personOn) {
            S.personOn = false;
            Log("[grasp] %s hand lets go of a person's part (%u solves, %u of them as it moved)\n", side ? "right" : "left",
                S.taken + S.resolved, S.resolved);
        }
        return true;
    }
    if (S.active && !S.person) return false;   // a thing's grasp
    if (!S.palm) return false;
    Pose inHand;
    std::copy_n(p, 3, inHand.p);
    std::copy_n(q, 4, inHand.q);
    VRIK_QuatNorm(inHand.q);
    const auto toSlot = [&](const Pose& h) {
        Pose o;
        float inv[4];
        ToFrame(S.pm.slot, h.p, o.p);
        VRIK_QuatConj(S.pm.slot.q, inv);
        VRIK_QuatMul(inv, h.q, o.q);
        VRIK_QuatNorm(o.q);
        return o;
    };
    Pose inSlot = toSlot(inHand);
    const double now = NowS();
    const bool fresh = !S.person || !S.personOn || S.style != style;
    if (!fresh) {
        // off its seat (the hold keeps it there: only a part that went somewhere else is solved again, where it is now)
        // by its geometry: its axis's way (either sense) and how far it is across it; along it only past 5 cm
        const float ez[3] = {0.0f, 0.0f, 1.0f};
        float d[3], a0[3], a1[3], across[3];
        Sub(inSlot.p, S.req.obj.p, d);
        Rot(S.req.obj.q, ez, a0);
        Rot(inSlot.q, ez, a1);
        const float along = Dot(d, a0);
        Mad(d, a0, -along, across);
        const float moved = Len(across) + std::max(0.0f, std::fabs(along) - 0.05f);
        const float turned = std::acos(std::min(1.0f, std::fabs(Dot(a0, a1))));
        if (moved < kPersonMoved && turned < kPersonTurned) {
            S.solvedAt = now;   // the time it has been off its seat counts from here
            return true;
        }
        if (now - S.solvedAt < kPersonEvery) return true;
        ++S.resolved;
    } else {
        ++S.taken;
        // seated as the grasp wants it, the physics hand moved to it (the caller); a part held round by an end of it is
        // under the palm instead
        Pose seat;
        int as = style;
        bool ok;
        // a part held round (an arm, a leg, a neck) lies across the palm, the fingers round it: on the palm instead, the
        // hand only lay on a limb (2026-11-11)
        if (palmGap >= 0.0f && style == 4) {
            ok = PersonOnPalm(S.pm, inHand, radius, halfLength, palmGap, seat);
        } else {
            ok = PersonSeat(S.pm, as, inHand, radius, halfLength, seat);
            if (!ok && as != 4) {
                as = 4;
                ok = palmGap >= 0.0f ? PersonOnPalm(S.pm, inHand, radius, halfLength, palmGap, seat)
                                     : PersonSeat(S.pm, as, inHand, radius, halfLength, seat);
            }
        }
        if (ok) {
            // at most kSeatMost: the rest of the way not taken (position and turn alike); on the palm along its normal, all
            float t[3];
            Sub(seat.p, inHand.p, t);
            const float dist = Len(t);
            if (palmGap < 0.0f && dist > kSeatMost) {
                const float k = kSeatMost / dist;
                Mad(inHand.p, t, k, seat.p);
                const float s = Dot4(inHand.q, seat.q) < 0.0f ? -1.0f : 1.0f;
                for (int c = 0; c < 4; ++c) seat.q[c] = inHand.q[c] + (seat.q[c] * s - inHand.q[c]) * k;
                VRIK_QuatNorm(seat.q);
            }
            style = as;
            inHand = seat;
            inSlot = toSlot(seat);
            if (seatP) std::copy_n(seat.p, 3, seatP);
            if (seatQ) std::copy_n(seat.q, 4, seatQ);
            if (seated) *seated = true;
        }
    }
    S.req = Request{};
    S.req.shape = kCapsule;
    S.req.half[0] = S.req.half[1] = std::max(0.005f, radius);
    S.req.half[2] = std::max(0.0f, halfLength);
    S.req.obj = inSlot;
    S.req.part = true;
    switch (style) {
    case 2: S.req.force = kWrap; S.req.spread = 0.15f; break;
    case 3: S.req.force = kWrap; S.req.limit = 0.6f; S.req.spread = 0.04f; break;
    case 4: S.req.force = kFlat; S.req.limit = 0.6f; S.req.spread = 0.06f; break;
    default: S.req.force = kWrap; break;
    }
    if (fresh) {
        static const char* names[5] = {"", "a can", "a can, the fingers wide", "gently", "the palm"};
        float skin[3];
        ToFrame(inHand, S.pm.skin, skin);   // the palm's skin in the part's frame
        Log("[grasp] %s hand closes on a person's part (capsule r %.3f, %.3f long, %.3f from the palm's skin) as %s\n",
            side ? "right" : "left", radius, 2.0f * halfLength, Sdf(kCapsule, S.req.half, skin), names[std::clamp(style, 1, 4)]);
        if (!S.person) S.blend = 0.0f;
        S.easedAt = 0.0;
    }
    S.style = asked;
    S.person = S.personOn = S.active = true;
    S.pin = false;
    S.solvedAt = now;
    ++S.rev;
    return true;
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
    float up[3];
    const bool upKnown = UpInHand(side, up);
    Pose seat;
    int kind = -1;
    if (!Seat(side, shape, h, at, in, upKnown ? up : nullptr, seat, &kind)) return;
    static const char* kKinds[8] = {"flat", "wrap", "ball", "pinch", "edge", "from above", "from below", "three fingers"};
    const volatile float* how = CyberpunkVR_DebugGraspApproach[side];
    Log("[grasp] %s hand seats shape %d (half %.3f %.3f %.3f) for a %s grasp; a long thing: palm to its axis %.2f, along it "
        "%.2f of its half, that end to up %.2f\n", side ? "right" : "left", shape, h[0], h[1], h[2],
        kind >= 0 && kind < 8 ? kKinds[kind] : "?", how[0], how[1], how[2]);
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
    S.req.force = -1;
    S.req.limit = 1.0f;
    S.req.spread = -1.0f;
    S.active = true;
    S.pin = true;
    ++S.rev;
    if (aOut) *aOut = true;
}

// VRGraspPin(side, pinned) -> Bool: whether the slot is held at the rig's reference while this grasp lasts (from VRGraspSet
// it is). A holster's weapon is not on the slot while the fingers close on it, and the weapon itself comes onto the slot
// while they still are: pinned, the game's animation cannot move the slot to that weapon's hold, and the weapon is
// seen 10-35 degrees off it until the grasp has gone.
void VRGraspPin(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t) {
    int32_t side = -1;
    bool pinned = true;
    RED4ext::GetParameter(aFrame, &side);
    RED4ext::GetParameter(aFrame, &pinned);
    aFrame->code++;
    if (aOut) *aOut = false;
    if (side < 0 || side > 1) return;
    Side& S = g_side[side];
    std::lock_guard<std::mutex> lk(S.lock);
    S.pin = pinned;
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
