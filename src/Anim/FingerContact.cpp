// FINGERS ON THE WORLD. A hand with a body of its own (Natives/PhysHands.cpp) stops at a table; its fingers bend on it,
// each joint about its own anatomical hinge and never past its range, the last joints giving before the knuckles. The
// solve runs on the physics hands' worker against the world's shapes round the player; this file only says where the
// fingers are and turns them by what comes back.
//
// WHERE THEY ARE: each finger's three bones in the hand bone's frame as every finger layer left them this pass (the
// resting hand, gestures, gloves), each joint's hinge -- the bone axis nearest the one that takes its segment toward the
// palm, as Anim/GraspPose.cpp finds it -- and how far it may turn from where it is now: its range measured from the rig's
// open hand. The chains are the ladder profile's slots (VRIK_LadderFingerBone): index..little proximal 6-9, middle
// 11-14, distal 15-18; the thumb's metacarpal 0 (not turned here: it sits in the palm), proximal 5, distal 10.
//
// WHAT COMES BACK is a turn per joint. The engine applies the player pose four or five times a tick and every pass of a
// tick must leave the same fingers, so the turns are taken once a batch (the same gap rule as the solve's clock in
// Hooks/AnimPose.cpp) and written again in every pass of it.
#include "Anim/FingerContact.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/Cyberware.hpp"
#include "Anim/GraspPose.hpp"
#include "Anim/LeftHanded.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/VrikState.hpp"
#include "Core/VrCoreShared.hpp"
#include "Natives/HeldBody.hpp"
#include "Natives/HandSkin.hpp"
#include "Natives/PhysHands.hpp"
#include "Utils/SharedSlots.hpp"
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstring>

extern "C" __declspec(dllexport) extern float CyberpunkVR_VrikBatchGapMs;

namespace cvr::anim {
namespace {
using cvr::physhands::FingerChain;
using cvr::physhands::FingerHand;

constexpr int kChains[5][3] = {{6, 11, 15}, {7, 12, 16}, {8, 13, 17}, {9, 14, 18}, {0, 5, 10}};
// The segments' capsules (m), Anim/GraspPose.cpp's; the thumb's metacarpal is in the palm and is not tested.
constexpr float kRadius[5][3] = {{0.0095f, 0.0085f, 0.0075f}, {0.0095f, 0.0085f, 0.0075f}, {0.0095f, 0.0085f, 0.0075f},
                                 {0.009f, 0.008f, 0.007f}, {0.0f, 0.0105f, 0.0095f}};
// EACH JOINT'S ANATOMICAL RANGE, degrees from the STRAIGHT finger (each bone in line with the one before, the first with
// the palm's own line), flexion positive. The knuckle (MCP) flexes to 90
// and gives back 30 under a load (passive extension 30-60); the middle joint (PIP) flexes to 105 (the little finger's to
// 110) and hardly past straight, its volar plate checking it (passive 0-15); the last joint (DIP) flexes to 85 and gives
// back 20 pressed on its pad (passive 0-20, a pulp pinch's hyperextended DIP). The thumb's CMC is not turned here; its
// MCP 55 / -10, its IP 80 / -20. Norkin & White, Measurement of Joint Motion (2009); AAOS (1965); Lin, Wu & Huang,
// "Modeling the constraints of human hand motion" (2000): MCP 0..90, PIP 0..110, DIP 0..90 actively.
constexpr float kLo[5][3] = {{-30, -8, -20}, {-30, -8, -20}, {-30, -8, -20}, {-30, -8, -20}, {0, -10, -20}};
constexpr float kHi[5][3] = {{90, 105, 85}, {90, 105, 85}, {90, 105, 85}, {90, 110, 85}, {0, 55, 80}};
constexpr float kDeg = 3.14159265f / 180.0f;

struct Pose { float q[4]{0, 0, 0, 1}; float p[3]{0, 0, 0}; };

bool Valid(int b) { return b >= 0 && b < g_VRBoneCount && b < VRIK_MAX_BONES; }
Pose Local(const uint8_t* buf, int bone) {
    Pose o;
    std::memcpy(o.p, buf + bone * 48 + VRIK_TRANS_OFF, sizeof(o.p));
    std::memcpy(o.q, buf + bone * 48 + VRIK_ROT_OFF, sizeof(o.q));
    VRIK_QuatNorm(o.q);
    return o;
}
Pose Compose(const Pose& a, const Pose& b) {
    Pose o;
    float r[3];
    VRIK_QuatRotateVec(a.q, b.p, r);
    for (int k = 0; k < 3; ++k) o.p[k] = a.p[k] + r[k];
    VRIK_QuatMul(a.q, b.q, o.q);
    VRIK_QuatNorm(o.q);
    return o;
}
// Bone b in the frame of its ancestor `root`.
bool InFrame(const uint8_t* buf, int bone, int root, Pose& out) {
    int chain[32], n = 0;
    for (int b = bone; b != root; b = g_VRBoneParent[b]) {
        if (!Valid(b) || n >= 32) return false;
        chain[n++] = b;
    }
    Pose acc;
    for (int i = n - 1; i >= 0; --i) acc = Compose(acc, Local(buf, chain[i]));
    out = acc;
    return true;
}
float Dot(const float* a, const float* b) { return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; }
void Cross(const float* a, const float* b, float* o) {
    o[0] = a[1] * b[2] - a[2] * b[1];
    o[1] = a[2] * b[0] - a[0] * b[2];
    o[2] = a[0] * b[1] - a[1] * b[0];
}
bool Unit(float* a) {
    const float l = std::sqrt(Dot(a, a));
    if (!(l > 1e-8f)) return false;
    for (int k = 0; k < 3; ++k) a[k] /= l;
    return true;
}
void TipOffset(const uint8_t* buf, int distal, float* tip) {
    for (int i = 0; i < g_VRBoneCount && i < VRIK_MAX_BONES; ++i) {
        if (g_VRBoneParent[i] != distal) continue;
        const Pose c = Local(buf, i);
        if (std::sqrt(Dot(c.p, c.p)) > 0.004f) { std::copy_n(c.p, 3, tip); return; }
    }
    const Pose d = Local(buf, distal);
    for (int k = 0; k < 3; ++k) tip[k] = 0.75f * d.p[k];
}
// THE HINGE IS ONE OF THE BONE'S OWN AXES: the one across the bone (y or z: the rig's finger bones run along x) nearest k
// (k in the hand's frame, the bone turned by q there). A finger joint is a hinge -- the interphalangeal joints do not bend
// sideways but a degree or two under a great force -- and the rig turns every finger bone about its own z. Kept as k when
// k was not within 32 deg of an axis, a finger whose bones are rolled against the palm (the little finger, the index)
// bent about a tilted axis, its last joints partly sideways (2026-11-09).
void LocalHinge(const float* q, const float* k, float* out) {
    float c[4];
    VRIK_QuatConj(q, c);
    VRIK_QuatRotateVec(c, k, out);
    const int best = std::fabs(out[2]) >= std::fabs(out[1]) ? 2 : 1;
    const float s = out[best] >= 0 ? 1.0f : -1.0f;
    out[0] = out[1] = out[2] = 0;
    out[best] = s;
}

// The palm's normal on the side the fingers close to, in the hand's frame (as Anim/GraspPose.cpp MeasurePalm finds it).
bool PalmNormal(const uint8_t* buf, int side, int hand, float* n) {
    const int index1 = VRIK_LadderFingerBone(side, 6), middle1 = VRIK_LadderFingerBone(side, 7),
              little1 = VRIK_LadderFingerBone(side, 9), middle2 = VRIK_LadderFingerBone(side, 12);
    for (int b : {index1, middle1, little1, middle2})
        if (!Valid(b)) return false;
    Pose i1, m1, l1;
    if (!InFrame(buf, index1, hand, i1) || !InFrame(buf, middle1, hand, m1) || !InFrame(buf, little1, hand, l1)) return false;
    float d[3] = {m1.p[0], m1.p[1], m1.p[2]}, across[3], t[3];
    if (!Unit(d)) return false;
    for (int k = 0; k < 3; ++k) across[k] = i1.p[k] - l1.p[k];
    const float along = Dot(across, d);
    for (int k = 0; k < 3; ++k) t[k] = across[k] - along * d[k];
    if (!Unit(t)) return false;
    Cross(d, t, n);
    if (!Unit(n)) return false;
    // the rig's flexion turns the middle finger's proximal phalanx about its own -Z: its next joint goes to the palm side.
    // Asked of the phalanx as the reference hand turns it (straight): as the animation turns it, a fist's knuckle past
    // 90 deg sent the next joint back the other way, the normal came out reversed, and every hinge with it (2026-11-09).
    Pose m1ref = m1;
    {
        Pose parent;
        float ref[4];
        if (InFrame(buf, g_VRBoneParent[middle1], hand, parent) && VRIK_ReferenceFingerRotation(middle1, ref)) {
            Pose l = Local(buf, middle1);
            std::copy_n(ref, 4, l.q);
            VRIK_QuatNorm(l.q);
            m1ref = Compose(parent, l);
        }
    }
    const Pose next = Local(buf, middle2);
    const float z[3] = {0, 0, -1};
    const float s = std::sin(10.0f * kDeg), bend[4] = {z[0] * s, z[1] * s, z[2] * s, std::cos(10.0f * kDeg)};
    float turned[4], a[3], b[3], moved[3];
    VRIK_QuatMul(m1ref.q, bend, turned);
    VRIK_QuatRotateVec(m1ref.q, next.p, a);
    VRIK_QuatRotateVec(turned, next.p, b);
    for (int k = 0; k < 3; ++k) moved[k] = b[k] - a[k];
    if (Dot(moved, n) < 0) for (int k = 0; k < 3; ++k) n[k] = -n[k];
    return true;
}

// EACH JOINT'S FLEXION FROM THE STRAIGHT FINGER, radians: the angle each bone makes with the one before about its
// hinge -- the first with the palm's own line, the wrist to the knuckle laid in the palm's plane (the hand frame's z, the
// palm's thickness, taken out: as PhysHands' open hand has it) -- the last bone's line to the rig's own fingertip, which is
// on the bone's line (the skin's is not). The chain posed from `base` by each bone's local rotation `q` and position.
void Flexion(const float* base, const float (*localQ)[4], const float (*local)[7], const float (*hinge)[3], const float* tip,
             float* out) {
    Pose acc, F[3];
    std::copy_n(base, 4, acc.q);
    std::copy_n(base + 4, 3, acc.p);
    float J[4][3];
    for (int j = 0; j < 3; ++j) {
        Pose l;
        std::copy_n(localQ[j], 4, l.q);
        std::copy_n(local[j] + 4, 3, l.p);
        acc = Compose(acc, l);
        F[j] = acc;
        std::copy_n(acc.p, 3, J[j]);
    }
    float r[3];
    VRIK_QuatRotateVec(F[2].q, tip, r);
    for (int k = 0; k < 3; ++k) J[3][k] = F[2].p[k] + r[k];
    float prev[3] = {J[0][0], J[0][1], 0.0f};
    for (int j = 0; j < 3; ++j) {
        float seg[3], cr[3], axis[3];
        for (int k = 0; k < 3; ++k) seg[k] = J[j + 1][k] - J[j][k];
        VRIK_QuatRotateVec(F[j].q, hinge[j], axis);
        Cross(prev, seg, cr);
        out[j] = std::atan2(Dot(cr, axis), Dot(prev, seg));
        if (!std::isfinite(out[j])) out[j] = 0;
        std::copy_n(seg, 3, prev);
    }
}

// for the trace's log: each joint's flexion as the animation has it and as the rig's reference hand has it, radians; and
// the animation's turn of each joint off its hinge (the lean and twist the hinge-only pose drops), radians
float flexAnim[2][5][3] = {}, flexRef[2][5][3] = {}, offHinge[2][5][3] = {};

void QuatAxisAngle(const float* axis, float a, float* q) {
    const float s = std::sin(0.5f * a);
    q[0] = axis[0] * s; q[1] = axis[1] * s; q[2] = axis[2] * s; q[3] = std::cos(0.5f * a);
}

// THE JOINT AS A HINGE: the local turn `anim` of a finger bone taken from the rig's straight reference `ref` and kept to
// its turn about the hinge k (the bone's own axis, in its frame) -- the swing-twist split of conj(ref)*anim about k. The
// knuckle (spread) keeps the rest's part about the axis across the hinge and the bone's line (its x): the fingers fan out
// as the animation spreads them; the twist about the bone's own line is dropped there too. Out: the pose, and the angle
// of what was dropped.
float HingeOnly(const float* ref, const float* anim, const float* k, bool knuckle, float* out) {
    float rc[4], D[4];
    VRIK_QuatConj(ref, rc);
    VRIK_QuatMul(rc, anim, D);
    if (D[3] < 0) for (auto& v : D) v = -v;
    const float pk = D[0] * k[0] + D[1] * k[1] + D[2] * k[2];
    float T[4] = {k[0] * pk, k[1] * pk, k[2] * pk, D[3]};
    const float tn = std::sqrt(T[0] * T[0] + T[1] * T[1] + T[2] * T[2] + T[3] * T[3]);
    if (!(tn > 1e-6f)) { T[0] = T[1] = T[2] = 0; T[3] = 1; }
    else for (auto& v : T) v /= tn;
    float Tc[4], S[4];
    VRIK_QuatConj(T, Tc);
    VRIK_QuatMul(D, Tc, S);   // D = S * T
    if (S[3] < 0) for (auto& v : S) v = -v;
    const float sn = std::sqrt(S[0] * S[0] + S[1] * S[1] + S[2] * S[2]);
    const float dropped = 2.0f * std::atan2(sn, S[3]);
    float keep[4] = {0, 0, 0, 1}, left = dropped;
    if (knuckle && sn > 1e-7f) {
        const float x[3] = {1, 0, 0};
        float y[3];
        Cross(k, x, y);
        if (Unit(y)) {
            float rv[3];
            for (int i = 0; i < 3; ++i) rv[i] = S[i] / sn * dropped;
            const float spread = Dot(rv, y);
            QuatAxisAngle(y, spread, keep);
            left = std::sqrt(std::max(0.0f, dropped * dropped - spread * spread));
        }
    }
    float a[4], b[4];
    VRIK_QuatMul(ref, keep, a);
    VRIK_QuatMul(a, T, b);
    VRIK_QuatNorm(b);
    std::copy_n(b, 4, out);
    return left;
}

// The palm's middle in the hand's frame, on its face: halfway from the wrist to the four knuckles, moved off the bones
// toward the palm's side -- what the thumb's joints bend toward.
bool PalmMiddle(const uint8_t* buf, int side, int hand, const float* n, float* out) {
    float sum[3] = {0, 0, 0};
    for (int slot = 6; slot <= 9; ++slot) {
        const int b = VRIK_LadderFingerBone(side, slot);
        Pose p;
        if (!Valid(b) || !InFrame(buf, b, hand, p)) return false;
        for (int k = 0; k < 3; ++k) sum[k] += p.p[k];
    }
    for (int k = 0; k < 3; ++k) out[k] = sum[k] * 0.125f + n[k] * 0.015f;
    return true;
}

// One finger's chain as drawn, its hinges and its ranges from where it is now. With `pure`, each joint's local turn kept
// to its hinge (HingeOnly) goes there and into the chain the solve is given; false in `pured` where it could not be made.
bool Chain(const uint8_t* buf, int side, int hand, int f, const float* n, const float* palmMiddle, FingerChain& c,
           float (*pure)[4] = nullptr, bool* pured = nullptr) {
    if (pured) *pured = false;
    int bone[3];
    for (int j = 0; j < 3; ++j) {
        bone[j] = VRIK_LadderFingerBone(side, kChains[f][j]);
        if (!Valid(bone[j])) return false;
    }
    Pose base;
    if (!InFrame(buf, g_VRBoneParent[bone[0]], hand, base)) return false;
    std::copy_n(base.q, 4, c.base);
    std::copy_n(base.p, 3, c.base + 4);
    Pose F[3], acc = base;
    for (int j = 0; j < 3; ++j) {
        const Pose l = Local(buf, bone[j]);
        std::copy_n(l.q, 4, c.local[j]);
        std::copy_n(l.p, 3, c.local[j] + 4);
        acc = Compose(acc, l);
        F[j] = acc;
    }
    TipOffset(buf, bone[2], c.tip);
    float rigTip[3];
    std::copy_n(c.tip, 3, rigTip);
    // THE SKIN (Natives/HandSkin.hpp): V's own hand -- each segment's width, its line off the bone, the fingertip
    const cvr::physhands::HandSkin* skin = cvr::physhands::SkinFor(g_VRBodyFemale, side);
    if (skin) std::copy_n(skin->tip[f], 3, c.tip);
    // THE HINGES FROM THE STRAIGHT FINGER: the chain posed by the rig's reference hand (its fingers straight), each
    // segment's line across the palm's normal, in that bone's own frame -- the same axis in the bone however the finger is
    // posed. Taken from the pose as drawn, a curled finger's middle and last segments stand along the palm's normal or
    // past it: a relaxed hand's middle finger (MCP 25, PIP 59, DIP 56 deg) had its middle joint's axis sideways and its
    // last joint's reversed, so the open hand read those joints as straight or bent back and left them curled, and the
    // solve turned them about the wrong axes (2026-11-09).
    float H[4][3];
    Pose R[3];
    {
        float refQ[3][4];
        bool all = true;
        for (int j = 0; j < 3 && all; ++j) all = VRIK_ReferenceFingerRotation(bone[j], refQ[j]);
        Pose a = base;
        for (int j = 0; j < 3; ++j) {
            Pose l;
            std::copy_n(all ? refQ[j] : c.local[j], 4, l.q);
            std::copy_n(c.local[j] + 4, 3, l.p);
            VRIK_QuatNorm(l.q);
            a = Compose(a, l);
            R[j] = a;
            std::copy_n(a.p, 3, H[j]);
        }
        float rr[3];
        VRIK_QuatRotateVec(R[2].q, all ? rigTip : c.tip, rr);
        for (int k = 0; k < 3; ++k) H[3][k] = R[2].p[k] + rr[k];
    }
    // THE THUMB'S joints bend toward the palm's middle (as Anim/GraspPose.cpp's thumb does), not toward the palm's
    // normal: the thumb lies turned against the fingers, its nail facing out from the palm's edge, and about the
    // fingers' axis its last joint bent sideways to its nail (2026-11-09). A thumb flexes and extends on its nail's plane.
    float toward[3];
    if (f == 4 && palmMiddle) {
        for (int k = 0; k < 3; ++k) toward[k] = palmMiddle[k] - H[1][k];
        if (!Unit(toward)) std::copy_n(n, 3, toward);
    } else {
        std::copy_n(n, 3, toward);
    }
    for (int j = 0; j < 3; ++j) {
        float seg[3], k[3];
        for (int a = 0; a < 3; ++a) seg[a] = H[j + 1][a] - H[j][a];
        Cross(seg, toward, k);
        if (!Unit(k)) return false;
        LocalHinge(R[j].q, k, c.hinge[j]);
        c.radius[j] = skin ? skin->radius[f][j] : kRadius[f][j];
        for (int i = 0; i < 3; ++i) c.offset[j][i] = skin ? skin->offset[f][j][i] : 0.0f;
        c.moves[j] = (f == 4 && j == 0) ? 0 : 1;
    }
    // THE JOINTS AS HINGES (physhands.ini finger_hinge_only): a straightened finger keeps none of the curled hand's lean
    // or twist off its hinges -- its last bone turned on its side on a wall where the pad should lie. The thumb's first
    // bone (in the palm) stays as the animation has it.
    {
        float refQ[3][4];
        bool all = true;
        for (int j = 0; j < 3 && all; ++j) all = VRIK_ReferenceFingerRotation(bone[j], refQ[j]);
        for (int j = 0; j < 3; ++j) {
            float P[4];
            if (all && !(f == 4 && j == 0)) {
                VRIK_QuatNorm(refQ[j]);
                offHinge[side][f][j] = HingeOnly(refQ[j], c.local[j], c.hinge[j], j == 0 || (f == 4 && j == 1), P);
            } else {
                std::copy_n(c.local[j], 4, P);
                offHinge[side][f][j] = 0;
            }
            if (pure && all) std::copy_n(P, 4, pure[j]);
        }
        if (pure && all) {
            for (int j = 0; j < 3; ++j) std::copy_n(pure[j], 4, c.local[j]);
            if (pured) *pured = true;
        }
    }
    // WHERE EACH JOINT IS IN ITS RANGE: its flexion from the straight finger now, measured on the bones' own lines about
    // the hinges above, so the range is the hand's anatomical one whatever pose the animation left the finger in.
    float q[3][4], flex[3];
    for (int j = 0; j < 3; ++j) std::copy_n(c.local[j], 4, q[j]);
    Flexion(c.base, q, c.local, c.hinge, rigTip, flex);
    for (int j = 0; j < 3; ++j) {
        c.lo[j] = std::min(0.0f, kLo[f][j] * kDeg - flex[j]);
        c.hi[j] = std::max(0.0f, kHi[f][j] * kDeg - flex[j]);
        c.flex[j] = flex[j];
        flexAnim[side][f][j] = flex[j];
    }
    {
        // the reference hand the same way, for the trace's log
        float ref[3][4], rf[3] = {0, 0, 0};
        bool all = true;
        for (int j = 0; j < 3 && all; ++j) all = VRIK_ReferenceFingerRotation(bone[j], ref[j]);
        if (all) Flexion(c.base, ref, c.local, c.hinge, rigTip, rf);
        std::copy_n(rf, 3, flexRef[side][f]);
    }
    return true;
}

double NowMs() {
    LARGE_INTEGER t, f;
    QueryPerformanceCounter(&t);
    QueryPerformanceFrequency(&f);
    return double(t.QuadPart) * 1000.0 / double(f.QuadPart);
}

double lastPassMs = 0;
float latched[2][5][3] = {}, latchedFlex[2][5][3] = {};
bool latchedOn[2] = {false, false}, latchedFlexKnown[2] = {false, false};
std::atomic<uint32_t> bendsRebased{0}, bendsClamped{0};
float weight[2] = {0, 0}, spentW[2] = {0, 0};
// THE TURNS AS DRAWN: eased toward what is solved, at most kTurnRate rad/s and over kTurnTau s, once a batch. A solve that
// found a finger in something put it out at once, and one that lost what a finger lay on let it go at once: the fingers
// jumped a quarter of their curl in a frame now and then. Kept as they were while the hand holds on (a person's part, a
// climb) once the grip has had kFreezeMs to close on it: the part moves under the hand and the solve with it, and the
// fingers on a held arm twitched with every step of it.
constexpr float kTurnRate = 9.0f, kTurnTau = 0.03f;
constexpr double kFreezeMs = 150.0;
constexpr float kBackMostDeg = 20.0f;
float drawnTurn[2][5][3] = {};
double holdSince[2] = {0, 0}, lastBatchMs = 0;
bool frozen[2] = {false, false};
std::atomic<uint32_t> turnsEased{0}, framesFrozen{0};
}  // namespace

void FingerContact(uint8_t* buf, const bool* busy) {
    if (!buf || g_VRBoneCount <= 0) return;
    const double now = NowMs();
    const bool newBatch = now - lastPassMs > double(CyberpunkVR_VrikBatchGapMs);
    lastPassMs = now;
    const int holding = cvr::heldbody::HoldingMask();
    for (int side = 0; side < 2; ++side) {
        const int hand = side ? g_VRRightBoneIdx : g_VRLeftBoneIdx;
        if (!Valid(hand)) continue;
        const bool on = cvr::physhands::On(side);
        FingerHand h{};
        h.at = GetTickCount64();
        // a gun out takes the hand that holds it (both, wielding two), not the other: that one lies on the gun's boxes and
        // on the world as an empty hand does
        const bool dual = g_pSharedHands && g_pSharedHands[vrshared::kDualWieldActive] > 0.5f;
        const bool gunHand = g_hasWeaponEquipped && (dual || side == (cvr::anim::WeaponInLeftHand() ? 0 : 1));
        const bool free = on && !(busy && busy[side]) && !GraspHolding(side) && !gunHand && !(holding & (1 << side)) &&
                          !VRIK_UsesNativeCyberware();
        // the chains always (the hand's body poses its finger capsules by them, a weapon's grip or not); the solve only
        // for a free hand
        float n[3];
        bool chained = PalmNormal(buf, side, hand, n);
        float middle[3];
        const bool haveMiddle = chained && PalmMiddle(buf, side, hand, n, middle);
        // the joints as hinges: the chain the solve is given and the pose drawn under the bends, both from the hinge-only
        // turns, so what is solved is what is drawn
        const bool hingeOnly = cvr::physhands::FingerHingeOnly();
        float pure[5][3][4];
        bool pured[5] = {false, false, false, false, false};
        for (int f = 0; f < 5 && chained; ++f)
            chained = Chain(buf, side, hand, f, n, haveMiddle ? middle : nullptr, h.f[f], hingeOnly ? pure[f] : nullptr, &pured[f]);
        h.free = free && chained;
        if (chained) cvr::physhands::PublishFingers(side, h);
        chained = h.free;
        // the turns: taken once a batch, eased in and out over a few
        if (newBatch) {
            float b[5][3], fx[5][3];
            const bool got = chained && cvr::physhands::FingerBends(side, b, fx);
            const bool holds = chained && cvr::physhands::HoldsOn(side);
            if (!holds) holdSince[side] = 0;
            else if (holdSince[side] == 0) holdSince[side] = now;
            frozen[side] = holds && latchedOn[side] && now - holdSince[side] > kFreezeMs;
            if (frozen[side]) framesFrozen.fetch_add(1, std::memory_order_relaxed);
            if (got && !frozen[side]) {
                std::memcpy(latched[side], b, sizeof(b));
                std::memcpy(latchedFlex[side], fx, sizeof(fx));
                latchedFlexKnown[side] = true;
            }
            latchedOn[side] = got || frozen[side];
            weight[side] += ((latchedOn[side] ? 1.0f : 0.0f) - weight[side]) * 0.35f;
            if (weight[side] < 0.002f) weight[side] = 0;
            spentW[side] += ((chained && cvr::physhands::GripSpent(side) ? 1.0f : 0.0f) - spentW[side]) * 0.3f;
            if (spentW[side] < 0.002f) spentW[side] = 0;
        }
        // trace=1, every two seconds: the right hand's fingers, each joint's flexion from straight (degrees) as the
        // animation has it, as the reference hand has it, its range, the turn taken and the result
        if (side == 1 && chained && newBatch) {
            static double toldAt = 0;
            if (now - toldAt > 2000.0 && cvr::physhands::Tracing()) {
                toldAt = now;
                char line[1500];
                int len = std::snprintf(line, sizeof(line), "[physhands] fingers (MCP PIP DIP from straight, deg; anim / ref / range / turn = drawn):");
                static const char* names[5] = {"index", "middle", "ring", "little", "thumb"};
                for (int f = 0; f < 5 && len > 0 && len < int(sizeof(line)) - 120; ++f) {
                    const FingerChain& c = h.f[f];
                    const float* a = flexAnim[1][f];
                    const float* r = flexRef[1][f];
                    const float* b = latched[1][f];
                    const float w = weight[1];
                    const float R = 1.0f / kDeg;
                    len += std::snprintf(line + len, sizeof(line) - len, " %s %.0f %.0f %.0f / %.0f %.0f %.0f / %.0f..%.0f %.0f..%.0f %.0f..%.0f / %.0f %.0f %.0f = %.0f %.0f %.0f;",
                                       names[f], a[0] * R, a[1] * R, a[2] * R, r[0] * R, r[1] * R, r[2] * R, (a[0] + c.lo[0]) * R,
                                       (a[0] + c.hi[0]) * R, (a[1] + c.lo[1]) * R, (a[1] + c.hi[1]) * R, (a[2] + c.lo[2]) * R,
                                       (a[2] + c.hi[2]) * R, b[0] * w * R, b[1] * w * R, b[2] * w * R, (a[0] + b[0] * w) * R,
                                       (a[1] + b[1] * w) * R, (a[2] + b[2] * w) * R);
                }
                len += std::snprintf(line + len, sizeof(line) - len, " bends re-based %u, clamped %u, eased %u, frames held still %u;",
                                     bendsRebased.load(), bendsClamped.load(), turnsEased.load(), framesFrozen.load());
                // the animation's turn off each hinge (the hinge-only pose drops it), degrees
                len += std::snprintf(line + len, sizeof(line) - len, " off hinge%s", hingeOnly ? " (dropped)" : "");
                for (int f = 0; f < 5 && len > 0 && len < int(sizeof(line)) - 24; ++f)
                    len += std::snprintf(line + len, sizeof(line) - len, " %.0f/%.0f/%.0f", offHinge[1][f][0] / kDeg,
                                         offHinge[1][f][1] / kDeg, offHinge[1][f][2] / kDeg);
                // each joint's hinge, the bone's own axis: y / z, upper case for +
                len += std::snprintf(line + len, sizeof(line) - len, " hinges");
                for (int f = 0; f < 5 && len > 0 && len < int(sizeof(line)) - 8; ++f) {
                    char code[4] = {0, 0, 0, 0};
                    for (int j = 0; j < 3; ++j) {
                        const float* x = h.f[f].hinge[j];
                        const int ax = std::fabs(x[2]) >= std::fabs(x[1]) ? 2 : 1;
                        code[j] = char((ax == 2 ? 'z' : 'y') - (x[ax] > 0 ? 32 : 0));
                    }
                    len += std::snprintf(line + len, sizeof(line) - len, " %s", code);
                }
                Log("%s\n", line);
            }
        }
        if ((weight[side] <= 0 && spentW[side] <= 0) || !chained) {
            std::memset(drawnTurn[side], 0, sizeof(drawnTurn[side]));
            continue;
        }
        const float dtB = float(std::clamp((now - lastBatchMs) / 1000.0, 0.001, 0.05));
        for (int f = 0; f < 5; ++f)
            for (int j = 0; j < 3; ++j) {
                float a = latched[side][f][j] * weight[side];
                // A BEND THAT HELD A FINGER OFF SOMETHING, ON AN ANIMATION THAT HAS OPENED SINCE (a grip let go with the
                // palm on a wall: the animation's fist turned straight at once, the turn back that kept the fist out of
                // the wall a few frames longer): what it kept was the finger's angle, so it is the less by how far the
                // animation opened, and never past straight the other way. And never out of the joint's range from the
                // pose it is put on: the fingers were thrown back past straight for a tenth of a second (2026-11-10).
                if (a < 0 && latchedFlexKnown[side]) {
                    const float opened = latchedFlex[side][f][j] - h.f[f].flex[j];
                    if (opened > 0) {
                        a = std::min(0.0f, a + opened);
                        bendsRebased.fetch_add(1, std::memory_order_relaxed);
                    }
                }
                // A HOLD PULLED OFF BY ITSELF (a person's part the controller was drawn away from): the grip counts as
                // opened, and the fingers open as if it were, however the grip is still held
                if (spentW[side] > 0 && f < 4) a += (-0.85f * h.f[f].flex[j] - a) * spentW[side];
                {
                    float& shown = drawnTurn[side][f][j];
                    if (newBatch) {
                        const float diff = a - shown, most = kTurnRate * dtB;
                        float step = diff * (1.0f - std::exp(-dtB / kTurnTau));
                        if (step > most || step < -most) {
                            step = std::clamp(step, -most, most);
                            turnsEased.fetch_add(1, std::memory_order_relaxed);
                        }
                        shown += step;
                    }
                    a = shown;
                }
                if (a < h.f[f].lo[j] || a > h.f[f].hi[j]) {
                    a = std::clamp(a, h.f[f].lo[j], h.f[f].hi[j]);
                    bendsClamped.fetch_add(1, std::memory_order_relaxed);
                }
                // and a knuckle or a middle joint never more than 20 degrees back past straight (a fingertip pressed on its
                // pad gives that much): a held person's part pressed into the palm had the solve throw a finger 27 back,
                // and the hold kept it so (2026-11-10)
                if (f < 4 && j < 2 && h.f[f].flex[j] + a < -kBackMostDeg * kDeg) {
                    a = -kBackMostDeg * kDeg - h.f[f].flex[j];
                    bendsClamped.fetch_add(1, std::memory_order_relaxed);
                }
                // a diagnostic, always on: a finger's knuckle or middle joint drawn more than 22 degrees back past
                // straight (the bend back a fingertip pressed on its pad takes is 20 at the most), rate-limited
                if (f < 4 && j < 2 && h.f[f].flex[j] + a < -22.0f * kDeg) {
                    static uint64_t toldAt[2] = {0, 0};
                    if (h.at - toldAt[side] > 250) {
                        toldAt[side] = h.at;
                        Log("[physhands] %s finger %d joint %d drawn %.0f deg past straight (animation %.0f, turn %.0f, latched %.0f at %.0f)\n",
                            side ? "right" : "left", f, j, -(h.f[f].flex[j] + a) / kDeg, h.f[f].flex[j] / kDeg, a / kDeg,
                            latched[side][f][j] / kDeg, latchedFlex[side][f][j] / kDeg);
                    }
                }
                if (a == 0 && !pured[f]) continue;
                const int bone = VRIK_LadderFingerBone(side, kChains[f][j]);
                if (!Valid(bone)) continue;
                const float* axis = h.f[f].hinge[j];
                const float s = std::sin(0.5f * a), turn[4] = {axis[0] * s, axis[1] * s, axis[2] * s, std::cos(0.5f * a)};
                float* q = reinterpret_cast<float*>(buf + bone * 48 + VRIK_ROT_OFF);
                float from[4];
                std::copy_n(q, 4, from);
                if (pured[f]) {
                    // eased onto the hinge-only pose as the bends are (a hand that leaves a wall gets its lean back)
                    const float* p = pure[f][j];
                    const float d = from[0] * p[0] + from[1] * p[1] + from[2] * p[2] + from[3] * p[3];
                    const float w = weight[side], sgn = d < 0 ? -1.0f : 1.0f;
                    for (int i = 0; i < 4; ++i) from[i] = from[i] * (1.0f - w) + p[i] * sgn * w;
                    VRIK_QuatNorm(from);
                }
                float r[4];
                VRIK_QuatMul(from, turn, r);
                VRIK_QuatNorm(r);
                std::copy_n(r, 4, q);
            }
    }
    if (newBatch) lastBatchMs = now;
}
}  // namespace cvr::anim
