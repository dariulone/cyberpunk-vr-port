#pragma once
// The holding hand for the pin bake and its report: the rig's finger chains (tools/grasp_tests/rig.txt),
// the hold poses (grenade/poses/*.lua), the palm's frame, the game's first-person hand mesh
// (tools/grasp_tests/mesh/hand_{l,r}.bin) skinned to a pose, and that skin's clearance to a grenade.
// All frames are the hand bone's. Both hands share local rotations; the right hand's offsets are negated.
#include "pin_math.hpp"
#include "pin_mesh.hpp"
#include <array>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <map>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace hand {
constexpr double kPi = 3.141592653589793;
constexpr double kDeg = kPi / 180;
constexpr int kSlots = 19;
// rig.txt's "fingers" order.
inline const char *const kSlotNames[kSlots] = {
    "InHandThumb", "InHandIndex", "InHandMiddle", "InHandRing", "InHandPinky",
    "HandThumb1",  "HandIndex1",  "HandMiddle1",  "HandRing1",  "HandPinky1",
    "HandThumb2",  "HandIndex2",  "HandMiddle2",  "HandRing2",  "HandPinky2",
    "HandIndex3",  "HandMiddle3", "HandRing3",    "HandPinky3"};
// Chains by slot: thumb, index, middle, ring, little finger.
inline const int kChain[5][4] = {{0, 5, 10, -1}, {1, 6, 11, 15}, {2, 7, 12, 16}, {3, 8, 13, 17}, {4, 9, 14, 18}};
// The twelve phalanges of the four fingers, the bones a grip key moves.
inline const int kFingerSlots[12] = {6, 11, 15, 7, 12, 16, 8, 13, 17, 9, 14, 18};
using Pose = std::array<Q, kSlots>;

inline Q axisAngle(V a, double radians) {
    a = unit(a);
    double s = std::sin(radians / 2);
    return {a.x * s, a.y * s, a.z * s, std::cos(radians / 2)};
}
inline Q fromTo(V a, V b) {
    a = unit(a);
    b = unit(b);
    V c = cross(a, b);
    double d = dot(a, b);
    if (d < -0.999999) {
        V o = std::abs(a.x) < .9 ? V{1, 0, 0} : V{0, 1, 0};
        return axisAngle(cross(a, o), kPi);
    }
    Q q{c.x, c.y, c.z, 1 + d};
    double n = std::sqrt(q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w);
    return {q.x / n, q.y / n, q.z / n, q.w / n};
}
// Signed angle of q about the unit axis (its twist part).
inline double twistAbout(Q q, V axis) {
    double p = q.x * axis.x + q.y * axis.y + q.z * axis.z;
    double a = 2 * std::atan2(p, q.w);
    return a > kPi ? a - 2 * kPi : a < -kPi ? a + 2 * kPi : a;
}
inline double angleBetween(Q a, Q b) {
    double d = std::abs(a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w);
    return 2 * std::acos(std::min(1., d));
}

struct Rig {
    std::map<int, int> parent;
    std::map<int, T> local;
    int bones[2][kSlots]{};
    int hand[2] = {23, 24};
    static Rig load(const std::filesystem::path &path) {
        std::ifstream f(path);
        if (!f)
            throw std::runtime_error("rig.txt missing");
        Rig r;
        std::string line;
        while (std::getline(f, line)) {
            std::istringstream s(line);
            std::string tag;
            s >> tag;
            if (tag == "fingers") {
                int side;
                s >> side;
                for (int i = 0; i < kSlots; ++i)
                    s >> r.bones[side][i];
            } else if (tag == "bone") {
                int bone, parent, one;
                T t;
                s >> bone >> parent >> one >> t.p.x >> t.p.y >> t.p.z >> t.q.x >> t.q.y >> t.q.z >> t.q.w;
                r.parent[bone] = parent;
                r.local[bone] = t;
            } else if (tag == "g_VRLeftBoneIdx") {
                s >> r.hand[0];
            } else if (tag == "g_VRRightBoneIdx") {
                s >> r.hand[1];
            }
        }
        return r;
    }
    Pose reference(int side) const {
        Pose p;
        for (int i = 0; i < kSlots; ++i)
            p[i] = local.at(bones[side][i]).q;
        return p;
    }
};

// A pose file: { 'LeftInHandThumb', x, y, z, w }, ... (LEFT names, parent-local).
inline Pose loadPose(const std::filesystem::path &path) {
    std::ifstream f(path);
    if (!f)
        throw std::runtime_error("pose missing: " + path.string());
    Pose p;
    std::array<bool, kSlots> seen{};
    std::string line;
    while (std::getline(f, line)) {
        auto a = line.find('\'');
        if (a == std::string::npos)
            continue;
        auto b = line.find('\'', a + 1);
        if (b == std::string::npos)
            continue;
        std::string name = line.substr(a + 1, b - a - 1);
        if (name.rfind("Left", 0) != 0)
            continue;
        name = name.substr(4);
        for (int i = 0; i < kSlots; ++i)
            if (name == kSlotNames[i]) {
                Q q;
                if (std::sscanf(line.c_str() + b + 1, " , %lf , %lf , %lf , %lf", &q.x, &q.y, &q.z, &q.w) != 4)
                    throw std::runtime_error("bad pose line: " + line);
                p[i] = q;
                seen[i] = true;
            }
    }
    for (int i = 0; i < kSlots; ++i)
        if (!seen[i])
            throw std::runtime_error(std::string("pose lacks ") + kSlotNames[i]);
    return p;
}

struct Frames {
    std::array<T, kSlots> bone;
    std::array<V, 5> tip;
};
// Each finger bone in the hand's frame, and each chain's tip (0.75 of the distal bone's own offset on).
inline Frames fk(const Rig &rig, int side, const Pose &pose) {
    Frames f;
    for (int i = 0; i < kSlots; ++i) {
        int bone = rig.bones[side][i], parent = rig.parent.at(bone);
        T base{};
        if (parent != rig.hand[side]) {
            int ps = -1;
            for (int j = 0; j < i; ++j)
                if (rig.bones[side][j] == parent)
                    ps = j;
            if (ps < 0)
                throw std::runtime_error("finger parent out of order");
            base = f.bone[ps];
        }
        f.bone[i] = compose(base, {rig.local.at(bone).p, pose[i]});
    }
    for (int c = 0; c < 5; ++c) {
        int last = kChain[c][c == 0 ? 2 : 3];
        V own = rig.local.at(rig.bones[side][last]).p;
        f.tip[c] = f.bone[last].p + rot(f.bone[last].q, own * .75);
    }
    return f;
}

// The palm (GraspPose.cpp's MeasurePalm at the rig's reference): d along the fingers, t across toward the
// index finger, n out of the palm (the way the fingers close).
struct Palm {
    V n, d, t, knuckle, skin;
};
inline Palm measurePalm(const Rig &rig, int side) {
    auto ref = rig.reference(side);
    auto f = fk(rig, side, ref);
    Palm pm;
    V i1 = f.bone[6].p, m1 = f.bone[7].p, r1 = f.bone[8].p, p1 = f.bone[9].p;
    pm.knuckle = (i1 + m1 + r1 + p1) * .25;
    pm.d = unit(m1);
    V across = i1 - p1;
    pm.t = unit(across - pm.d * dot(across, pm.d));
    pm.n = unit(cross(pm.d, pm.t));
    Pose bent = ref;
    bent[7] = ref[7] * axis(2, -20);
    auto g = fk(rig, side, bent);
    if (dot(g.bone[12].p - f.bone[12].p, pm.n) < 0)
        pm.n = pm.n * -1;
    pm.skin = pm.knuckle * .5 + pm.n * .014;
    return pm;
}

// A bone's bend in its parent's frame: flexion about local -Z, abduction about local +Y, twist about the
// bone's own axis (degrees). `handedness` is +1 on the left hand (bones along +X), -1 on the right.
struct Bend {
    double flex, abd, twist;
};
inline Bend bendOf(Q q, double handedness) {
    V dir = rot(q, {handedness, 0, 0}) * handedness;
    Bend b;
    b.flex = std::atan2(-dir.y, dir.x) / kDeg;
    b.abd = std::asin(std::clamp(-dir.z, -1., 1.)) / kDeg;
    // The twist left after the least swing from the bone's axis.
    Q swing = fromTo({handedness, 0, 0}, rot(q, {handedness, 0, 0}));
    b.twist = twistAbout(inv(swing) * q, {1, 0, 0}) / kDeg;
    return b;
}

// THE THUMB'S ANATOMY (GraspPose.cpp's SolveThumb): the metacarpal's elevation off the palm and its angle
// out from the fingers' way, a roll coupled to opposition with free play, the MCP's flexion and a little
// abduction, the IP's flexion. Ranges are degrees.
constexpr double kRollPerLift = .45, kRollFree = 15;
constexpr double kElevEase[2] = {-5, 70}, kAwayEase[2] = {-25, 65};
constexpr double kElevHard[2] = {-20, 80}, kAwayHard[2] = {-50, 85};
constexpr double kMcpEase[2] = {10, 50}, kMcpHard[2] = {0, 65};
constexpr double kAbdEase[2] = {-8, 8}, kAbdHard[2] = {-14, 14};
constexpr double kIpEase[2] = {-5, 70}, kIpHard[2] = {-15, 80};
constexpr double kWholeAway = 75;
inline double past(double v, const double *r) {
    return v < r[0] ? r[0] - v : v > r[1] ? v - r[1] : 0;
}

struct Thumb {
    Palm pm;
    V meta0, rollAxis, base;
    double elev0 = 0, away0 = 0, handedness = 1;
    Q rest[3];
    Bend mcp0, ip0;
    // rest: the thumb's three local rotations the angles are taken from (the hold's).
    static Thumb make(const Rig &rig, int side, const Pose &pose) {
        Thumb t;
        t.pm = measurePalm(rig, side);
        t.handedness = side == 0 ? 1 : -1;
        auto f = fk(rig, side, pose);
        t.base = f.bone[0].p;
        t.meta0 = unit(f.bone[5].p - f.bone[0].p);
        bool front = dot(cross(t.pm.d, t.pm.t), t.pm.n) > 0;
        t.rollAxis = front ? t.meta0 * -1 : t.meta0;
        t.elev0 = std::asin(std::clamp(dot(t.meta0, t.pm.n), -1., 1.)) / kDeg;
        t.away0 = std::atan2(dot(t.meta0, t.pm.t), dot(t.meta0, t.pm.d)) / kDeg;
        t.rest[0] = pose[0];
        t.rest[1] = pose[5];
        t.rest[2] = pose[10];
        t.mcp0 = bendOf(pose[5], t.handedness);
        t.ip0 = bendOf(pose[10], t.handedness);
        return t;
    }
    double rollWith(double elev, double away) const {
        return kRollPerLift * ((elev - elev0) + .5 * (away0 - away));
    }
    // Deltas from the rest: elevation, angle out, free roll, MCP flexion, MCP abduction, IP flexion.
    void locals(const double *p, Q out[3]) const {
        double e = (elev0 + p[0]) * kDeg, a = (away0 + p[1]) * kDeg;
        V want = pm.n * std::sin(e) + (pm.d * std::cos(a) + pm.t * std::sin(a)) * std::cos(e);
        double roll = (rollWith(elev0 + p[0], away0 + p[1]) + p[2]) * kDeg;
        out[0] = fromTo(meta0, want) * axisAngle(rollAxis, roll) * rest[0];
        out[1] = rest[1] * axis(1, p[4]) * axis(2, -p[3]);
        out[2] = rest[2] * axis(2, -p[5]);
    }
    struct Angles {
        double elev, away, roll, mcp, abd, twist, ip, whole;
    };
    // The anatomical reading of any three thumb locals.
    Angles read(const Rig &rig, int side, const Q q[3]) const {
        Pose p = rig.reference(side);
        p[0] = q[0];
        p[5] = q[1];
        p[10] = q[2];
        auto f = fk(rig, side, p);
        V meta = unit(f.bone[5].p - f.bone[0].p);
        Angles a;
        a.elev = std::asin(std::clamp(dot(meta, pm.n), -1., 1.)) / kDeg;
        a.away = std::atan2(dot(meta, pm.t), dot(meta, pm.d)) / kDeg;
        Q swing = fromTo(meta0, meta);
        Q left = inv(swing) * q[0] * inv(rest[0]);
        a.roll = twistAbout(left, rollAxis) / kDeg - rollWith(a.elev, a.away);
        Bend m = bendOf(q[1], handedness), i = bendOf(q[2], handedness);
        a.mcp = m.flex;
        a.abd = m.abd;
        a.twist = m.twist;
        a.ip = i.flex;
        V whole = unit(f.tip[0] - f.bone[0].p);
        a.whole = std::atan2(dot(whole, pm.t), dot(whole, pm.d)) / kDeg;
        return a;
    }
    // Soft walls past a real thumb's ease (per degree squared) and the free roll's play. The MCP's
    // abduction is read off the game's own hold (its frame carries some 20 degrees of it at rest).
    double strain(const Angles &a) const {
        double r = std::max(0., std::abs(a.roll) - kRollFree);
        double s = past(a.elev, kElevEase), w = past(a.away, kAwayEase), m = past(a.mcp, kMcpEase),
               b = past(a.abd - mcp0.abd, kAbdEase), i = past(a.ip, kIpEase),
               h = std::max(0., std::abs(a.whole) - kWholeAway);
        return s * s + w * w + m * m + b * b + i * i + r * r + h * h;
    }
    bool withinHard(const Angles &a) const {
        return !past(a.elev, kElevHard) && !past(a.away, kAwayHard) && !past(a.mcp, kMcpHard) &&
               !past(a.abd - mcp0.abd, kAbdHard) && !past(a.ip, kIpHard) && std::abs(a.roll) <= 2 * kRollFree + .5;
    }
};

// ---------------------------------------------------------------- the game's hand mesh

struct M4 {
    double m[16]{}; // column-major
    static M4 identity() {
        M4 r;
        r.m[0] = r.m[5] = r.m[10] = r.m[15] = 1;
        return r;
    }
    static M4 from(const T &t) {
        V c0 = rot(t.q, {1, 0, 0}), c1 = rot(t.q, {0, 1, 0}), c2 = rot(t.q, {0, 0, 1});
        M4 r;
        double v[16] = {c0.x, c0.y, c0.z, 0, c1.x, c1.y, c1.z, 0, c2.x, c2.y, c2.z, 0, t.p.x, t.p.y, t.p.z, 1};
        std::memcpy(r.m, v, sizeof v);
        return r;
    }
    V point(V p) const {
        return {m[0] * p.x + m[4] * p.y + m[8] * p.z + m[12], m[1] * p.x + m[5] * p.y + m[9] * p.z + m[13],
                m[2] * p.x + m[6] * p.y + m[10] * p.z + m[14]};
    }
    V dir(V p) const {
        return {m[0] * p.x + m[4] * p.y + m[8] * p.z, m[1] * p.x + m[5] * p.y + m[9] * p.z,
                m[2] * p.x + m[6] * p.y + m[10] * p.z};
    }
};
inline M4 operator*(const M4 &a, const M4 &b) {
    M4 o;
    for (int c = 0; c < 4; ++c)
        for (int r = 0; r < 4; ++r) {
            double s = 0;
            for (int k = 0; k < 4; ++k)
                s += a.m[k * 4 + r] * b.m[c * 4 + k];
            o.m[c * 4 + r] = s;
        }
    return o;
}
// Affine inverse (the 3x3 part inverted in general, then the translation).
inline M4 inverse(const M4 &a) {
    const double *m = a.m;
    // m[c*4+r]; cofactors of the 3x3 block.
    double a00 = m[0], a01 = m[4], a02 = m[8], a10 = m[1], a11 = m[5], a12 = m[9], a20 = m[2], a21 = m[6],
           a22 = m[10];
    double c00 = a11 * a22 - a12 * a21, c01 = a12 * a20 - a10 * a22, c02 = a10 * a21 - a11 * a20;
    double det = a00 * c00 + a01 * c01 + a02 * c02;
    if (std::abs(det) < 1e-15)
        throw std::runtime_error("singular bind");
    double i00 = c00 / det, i01 = (a02 * a21 - a01 * a22) / det, i02 = (a01 * a12 - a02 * a11) / det;
    double i10 = c01 / det, i11 = (a00 * a22 - a02 * a20) / det, i12 = (a02 * a10 - a00 * a12) / det;
    double i20 = c02 / det, i21 = (a01 * a20 - a00 * a21) / det, i22 = (a00 * a11 - a01 * a10) / det;
    M4 o;
    o.m[0] = i00, o.m[4] = i01, o.m[8] = i02;
    o.m[1] = i10, o.m[5] = i11, o.m[9] = i12;
    o.m[2] = i20, o.m[6] = i21, o.m[10] = i22;
    for (int r = 0; r < 3; ++r)
        o.m[12 + r] = -(o.m[r] * m[12] + o.m[4 + r] * m[13] + o.m[8 + r] * m[14]);
    o.m[15] = 1;
    return o;
}

// 0 palm and the rest, 1 thumb, 2 index, 3 middle, 4 ring, 5 little finger.
inline int fingerOf(const std::string &n) {
    auto has = [&](const char *s) { return n.find(s) != std::string::npos; };
    if (has("Thumb") || has("_thumb"))
        return 1;
    if (has("Index") || has("_ind_"))
        return 2;
    if (has("Middle") || has("_mid_"))
        return 3;
    if (has("Ring") || has("_ring_"))
        return 4;
    if (has("Pinky") || has("_pinky"))
        return 5;
    return 0;
}

// The first-person hand (handmesh.py's HMS1), in the hand bone's frame. Muscle joints follow two bones half
// way, as tools/grasp_tests/handmesh.hpp drives them; joints off the hand stay with it.
struct HandMesh {
    int side = 0;
    std::vector<std::string> names;
    std::vector<M4> bind;
    std::vector<int> a, b; // slots followed (-1: the hand)
    std::vector<double> mix;
    std::vector<char> onHand;
    std::vector<V> v, n;
    std::vector<std::array<uint16_t, 8>> j;
    std::vector<std::array<float, 8>> w;
    std::vector<uint32_t> tri;
    std::vector<char> draw, used;
    std::vector<int> group;
    std::map<int, M4> rigBind;
    std::vector<V> sv, sn;
    std::vector<int> byGroup[6];

    static HandMesh load(const std::filesystem::path &path, int side, const Rig &rig) {
        std::ifstream f(path, std::ios::binary);
        if (!f)
            throw std::runtime_error("hand mesh missing: " + path.string());
        char magic[4];
        uint32_t nj, nv, nt;
        f.read(magic, 4);
        f.read(reinterpret_cast<char *>(&nj), 4);
        f.read(reinterpret_cast<char *>(&nv), 4);
        f.read(reinterpret_cast<char *>(&nt), 4);
        if (std::memcmp(magic, "HMS1", 4) != 0)
            throw std::runtime_error("not HMS1");
        HandMesh M;
        M.side = side;
        std::vector<M4> ibm(nj);
        M.names.resize(nj);
        for (uint32_t i = 0; i < nj; ++i) {
            char nm[64];
            float m[16];
            f.read(nm, 64);
            nm[63] = 0;
            M.names[i] = nm;
            f.read(reinterpret_cast<char *>(m), 64);
            for (int k = 0; k < 16; ++k)
                ibm[i].m[k] = m[k];
        }
        struct Raw {
            float p[3], n[3];
            uint16_t j[8];
            float w[8];
        };
        std::vector<Raw> raw(nv);
        f.read(reinterpret_cast<char *>(raw.data()), sizeof(Raw) * nv);
        M.tri.resize(size_t(nt) * 3);
        f.read(reinterpret_cast<char *>(M.tri.data()), 4 * M.tri.size());
        if (!f)
            throw std::runtime_error("short hand mesh");
        const std::string pre = side ? "Right" : "Left", mp = side ? "r_" : "l_";
        int handJoint = -1;
        for (uint32_t i = 0; i < nj; ++i)
            if (M.names[i] == pre + "Hand")
                handJoint = int(i);
        if (handJoint < 0)
            throw std::runtime_error("no hand joint");
        // glTF's Y-up back to the game's Z-up: game (x, y, z) = glTF (x, -z, y).
        M4 K = M4::identity(), Kt = M4::identity();
        K.m[5] = 0, K.m[6] = 1, K.m[9] = -1, K.m[10] = 0;
        Kt.m[5] = 0, Kt.m[6] = -1, Kt.m[9] = 1, Kt.m[10] = 0;
        const M4 bhInv = ibm[handJoint];
        const M4 toGame = K * bhInv;
        M.bind.resize(nj);
        for (uint32_t i = 0; i < nj; ++i)
            M.bind[i] = K * bhInv * inverse(ibm[i]) * Kt;
        auto slotOf = [&](const std::string &name) {
            if (name.rfind(pre, 0) != 0)
                return -2;
            std::string rest = name.substr(pre.size());
            for (int s = 0; s < kSlots; ++s)
                if (rest == kSlotNames[s])
                    return s;
            return rest == "Hand" ? -1 : -2;
        };
        auto slotNamed = [&](const char *s) {
            for (int k = 0; k < kSlots; ++k)
                if (std::strcmp(kSlotNames[k], s) == 0)
                    return k;
            return -1;
        };
        M.a.assign(nj, -1);
        M.b.assign(nj, -1);
        M.mix.assign(nj, 0);
        M.onHand.assign(nj, 0);
        for (uint32_t i = 0; i < nj; ++i) {
            const std::string &nm = M.names[i];
            int s = slotOf(nm);
            if (s >= -1) {
                M.a[i] = M.b[i] = s;
                M.onHand[i] = 1;
                continue;
            }
            const struct {
                const char *part, *from, *to;
                double k;
            } rules[] = {{"_ind_knuckle", "InHandIndex", "HandIndex1", .5},
                         {"_mid_knuckle", "InHandMiddle", "HandMiddle1", .5},
                         {"_ring_knuckle", "InHandRing", "HandRing1", .5},
                         {"_pinky_knuckle", "InHandPinky", "HandPinky1", .5},
                         {"_pinkyBase", "InHandPinky", "HandPinky1", .25},
                         {"_thumb_side", "", "InHandThumb", .5},
                         {"_thumb_thumb_A", "", "InHandThumb", .5}};
            for (auto &r : rules)
                if (nm.rfind(mp, 0) == 0 && nm.find(r.part) != std::string::npos) {
                    M.a[i] = r.from[0] ? slotNamed(r.from) : -1;
                    M.b[i] = r.to[0] ? slotNamed(r.to) : -1;
                    M.mix[i] = r.k;
                    M.onHand[i] = 1;
                    break;
                }
        }
        // Each slot's bind in the hand's bind frame: its joint's, or (the thumb's metacarpal, which the mesh
        // lacks) its child's with the rig's reference between them.
        auto ref = fk(rig, side, rig.reference(side));
        auto refOf = [&](int s) { return M4::from(ref.bone[s]); };
        for (int s = 0; s < kSlots; ++s) {
            for (uint32_t i = 0; i < nj; ++i)
                if (slotOf(M.names[i]) == s)
                    M.rigBind[s] = M.bind[i];
            if (M.rigBind.count(s))
                continue;
            for (int c = 0; c < kSlots && !M.rigBind.count(s); ++c)
                if (rig.parent.at(rig.bones[side][c]) == rig.bones[side][s])
                    for (uint32_t i = 0; i < nj; ++i)
                        if (slotOf(M.names[i]) == c && !M.rigBind.count(s))
                            M.rigBind[s] = M.bind[i] * inverse(refOf(c)) * refOf(s);
            if (!M.rigBind.count(s))
                M.rigBind[s] = refOf(s);
        }
        M.v.resize(nv);
        M.n.resize(nv);
        M.j.resize(nv);
        M.w.resize(nv);
        M.group.resize(nv);
        std::vector<double> handW(nv);
        for (uint32_t k = 0; k < nv; ++k) {
            M.v[k] = toGame.point({raw[k].p[0], raw[k].p[1], raw[k].p[2]});
            M.n[k] = unit(toGame.dir({raw[k].n[0], raw[k].n[1], raw[k].n[2]}));
            double hw = 0, best = -1;
            int heavy = 0;
            for (int s = 0; s < 8; ++s) {
                bool ok = raw[k].j[s] < nj;
                M.j[k][s] = ok ? raw[k].j[s] : 0;
                M.w[k][s] = ok ? raw[k].w[s] : 0.f;
                if (M.onHand[M.j[k][s]])
                    hw += M.w[k][s];
                if (M.w[k][s] > best) {
                    best = M.w[k][s];
                    heavy = M.j[k][s];
                }
            }
            handW[k] = hw;
            M.group[k] = fingerOf(M.names[heavy]);
        }
        M.draw.resize(nt);
        M.used.assign(nv, 0);
        for (uint32_t t = 0; t < nt; ++t) {
            double most = std::max({handW[M.tri[t * 3]], handW[M.tri[t * 3 + 1]], handW[M.tri[t * 3 + 2]]});
            M.draw[t] = most >= .5;
            if (M.draw[t])
                for (int c = 0; c < 3; ++c)
                    M.used[M.tri[t * 3 + c]] = 1;
        }
        for (uint32_t k = 0; k < nv; ++k)
            if (M.used[k])
                M.byGroup[M.group[k]].push_back(int(k));
        return M;
    }
    // The bind against the rig: the index metacarpal's and the little finger's knuckle place (m).
    double bindError(const Rig &rig) const {
        auto ref = fk(rig, side, rig.reference(side));
        double worst = 0;
        for (int s : {1, 9}) {
            const M4 &q = rigBind.at(s);
            V d = V{q.m[12], q.m[13], q.m[14]} - ref.bone[s].p;
            worst = std::max(worst, std::sqrt(dot(d, d)));
        }
        return worst;
    }
    // Skinned to `pose`; `only` limits the work to one group's vertices (-1: all).
    void skin(const Rig &rig, const Pose &pose, int only = -1) {
        auto f = fk(rig, side, pose);
        M4 skinOf[kSlots];
        for (int s = 0; s < kSlots; ++s)
            skinOf[s] = M4::from(f.bone[s]) * inverse(rigBind.at(s));
        const M4 id = M4::identity();
        std::vector<M4> S(names.size());
        for (size_t i = 0; i < names.size(); ++i) {
            if (!onHand[i]) {
                S[i] = id;
                continue;
            }
            const M4 &A = a[i] < 0 ? id : skinOf[a[i]], &B = b[i] < 0 ? id : skinOf[b[i]];
            for (int k = 0; k < 16; ++k)
                S[i].m[k] = A.m[k] + (B.m[k] - A.m[k]) * mix[i];
        }
        if (sv.size() != v.size()) {
            sv.assign(v.size(), {});
            sn.assign(v.size(), {});
        }
        auto one = [&](int k) {
            V p{}, nn{};
            for (int s = 0; s < 8; ++s) {
                double wt = w[k][s];
                if (wt <= 0)
                    continue;
                p = p + S[j[k][s]].point(v[k]) * wt;
                nn = nn + S[j[k][s]].dir(n[k]) * wt;
            }
            sv[k] = p;
            double l = std::sqrt(dot(nn, nn));
            sn[k] = l > 0 ? nn * (1 / l) : V{0, 0, 1};
        };
        if (only >= 0) {
            for (int k : byGroup[only])
                one(k);
        } else {
            for (size_t k = 0; k < v.size(); ++k)
                one(int(k));
        }
    }
};

// How much of a vertex a finger's own phalanges carry (its knuckle's muscles half, the palm none).
inline std::vector<double> mobility(const HandMesh &M, int chain) {
    std::vector<double> joint(M.names.size(), 0);
    auto moves = [&](int slot) {
        for (int k = 1; k < 4; ++k)
            if (kChain[chain][k] == slot)
                return true;
        return false;
    };
    for (size_t i = 0; i < M.names.size(); ++i) {
        if (!M.onHand[i])
            continue;
        double a = M.a[i] >= 0 && moves(M.a[i]) ? 1 : 0, b = M.b[i] >= 0 && moves(M.b[i]) ? 1 : 0;
        joint[i] = a + (b - a) * M.mix[i];
    }
    std::vector<double> out(M.v.size(), 0);
    for (size_t k = 0; k < M.v.size(); ++k)
        for (int s = 0; s < 8; ++s)
            out[k] += M.w[k][s] * joint[M.j[k][s]];
    return out;
}
// The skin each finger moves (moving[1..4]: index..little, the vertices its phalanges carry at least half of)
// and the skin no finger moves (the palm, the knuckles' heels; the thumb's own is apart).
struct Partition {
    std::array<std::vector<int>, 5> moving;
    std::vector<int> still;
};
inline Partition partition(const HandMesh &M) {
    Partition p;
    std::array<std::vector<double>, 5> mob;
    for (int c = 1; c < 5; ++c)
        mob[c] = mobility(M, c);
    for (int g = 0; g < 6; ++g)
        for (int k : M.byGroup[g]) {
            if (g == 1)
                continue;
            if (g >= 2 && mob[g - 1][k] >= .5)
                p.moving[g - 1].push_back(k);
            else
                p.still.push_back(k);
        }
    return p;
}

// The skin's clearance to the body held at `prop` (m; under 0 inside), per group, over the skinned vertices.
struct Clearance {
    std::array<double, 6> least{};
    std::array<int, 6> inside{};
};
inline Clearance clearance(const HandMesh &M, const PinBody &body, const T &prop, bool exact = false,
                           int only = -1) {
    Clearance c;
    c.least.fill(1e9);
    c.inside.fill(0);
    Q qi = inv(prop.q);
    for (int g = 0; g < 6; ++g) {
        if (only >= 0 && g != only)
            continue;
        for (int k : M.byGroup[g]) {
            V l = rot(qi, M.sv[k] - prop.p);
            double d = exact ? body.distanceExact(l) : body.distance(l);
            c.least[g] = std::min(c.least[g], d);
            if (d < 0)
                ++c.inside[g];
        }
    }
    return c;
}
} // namespace hand
