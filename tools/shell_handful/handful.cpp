// handful.cpp -- a pump's HANDFUL of shells in the left hand, and the move of the next one into the fingers.
//
// 2026-11-06: the shell in the fingers already sits right -- the pose the hand pushes it into the port
// with -- but the rest of a handful lay in a straight row out of the fist. Fit them to that pose, and after each
// shell goes in let the hand roll the next one into the same pose.
//
// The hand is the live plugin's reference left hand (tools/grasp_tests/rig.txt) wearing the gun's carry pose, in the
// weapon slot's frame (bone 26, WeaponLeft), modelled as the lobby's grasp solver models it (src/Anim/GraspPose.cpp):
// finger capsules, the palm's slab 14 mm deep over the plane of the bones, flexion about each joint's own -Z.
//
// The carry holds shell 1 like a cigar: between the thumb's pad and the side of the curled index finger, pointing
// along the fingers. Nothing parallel to it fits in that hand -- the middle, ring and little fingers are hooks across
// its length -- so the rest lie where a hand keeps them while the thumb feeds: ACROSS the palm in the fist, brass
// toward the index finger, and the middle, ring and little fingers close on them.
//
//   1. Shell 1 is the carry frame, untouched: the pose and the push stay what they were.
//   2. The others, one orientation for all (the carry's turned a quarter about the palm's normal), placed one by one
//      into the fist: on the palm or on the ones before, as close to the grip's line as stays out of the thumb, the
//      index finger, shell 1 and the other fingers held straight.
//   3. Per count c (shells in the hand) the middle, ring and little fingers close from straight onto shells 2..c, every
//      joint together, a link that touches stopping its own joint and those before it (GraspIt's auto-grasp). With
//      shell 1 alone they are the carry's again.
//   4. The move from c to c - 1, the one placed last (on top) first: the fist eases off, the shell comes up out of it,
//      turns the quarter back and lands in the carry frame exactly; the thumb and the index finger make way over the
//      middle of it; the fist closes on what is left. Its lift and the fingers' give are the cheapest that keep the
//      shell out of the hand all the way.
//
// Out: <out>.lua (reload/handful/<gun>.lua: holds, poses, move keys) and <out>_*.ppm (orthographic views).
//
//   handful.exe <in.txt> <out prefix> [--dump]
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <functional>
#include <map>
#include <sstream>
#include <string>
#include <vector>

namespace {

bool gTrace = false;   // --trace: every finger link that stops closing, and on what
constexpr double kPi = 3.14159265358979323846;
constexpr double kDeg = kPi / 180.0;

struct V3 {
    double x = 0, y = 0, z = 0;
};
V3 operator+(V3 a, V3 b) { return {a.x + b.x, a.y + b.y, a.z + b.z}; }
V3 operator-(V3 a, V3 b) { return {a.x - b.x, a.y - b.y, a.z - b.z}; }
V3 operator*(V3 a, double s) { return {a.x * s, a.y * s, a.z * s}; }
double Dot(V3 a, V3 b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
V3 Cross(V3 a, V3 b) { return {a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x}; }
double Len(V3 a) { return std::sqrt(Dot(a, a)); }
V3 Unit(V3 a) {
    const double l = Len(a);
    return l > 1e-12 ? a * (1.0 / l) : V3{0, 0, 1};
}
V3 Lerp(V3 a, V3 b, double t) { return a + (b - a) * t; }

struct Q {
    double x = 0, y = 0, z = 0, w = 1;
};
Q Mul(Q a, Q b) {
    return {a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y, a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
            a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w, a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z};
}
Q Conj(Q q) { return {-q.x, -q.y, -q.z, q.w}; }
Q Norm(Q q) {
    const double l = std::sqrt(q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w);
    return {q.x / l, q.y / l, q.z / l, q.w / l};
}
V3 Rot(Q q, V3 v) {
    const V3 u{q.x, q.y, q.z};
    const V3 t = Cross(u, v) * 2.0;
    return v + t * q.w + Cross(u, t);
}
Q AxisAngle(V3 axis, double a) {
    const V3 n = Unit(axis);
    const double s = std::sin(0.5 * a);
    return {n.x * s, n.y * s, n.z * s, std::cos(0.5 * a)};
}
Q Nlerp(Q a, Q b, double t) {
    if (a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w < 0) b = {-b.x, -b.y, -b.z, -b.w};
    return Norm({a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t, a.w + (b.w - a.w) * t});
}
// Spherical, so a quarter turn goes round at an even rate.
Q Slerp(Q a, Q b, double t) {
    double c = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
    if (c < 0) b = {-b.x, -b.y, -b.z, -b.w}, c = -c;
    if (c > 0.9995) return Nlerp(a, b, t);
    const double th = std::acos(c), s = std::sin(th);
    const double wa = std::sin((1 - t) * th) / s, wb = std::sin(t * th) / s;
    return Norm({a.x * wa + b.x * wb, a.y * wa + b.y * wb, a.z * wa + b.z * wb, a.w * wa + b.w * wb});
}

struct Fr {
    V3 p;
    Q q;
};
Fr Compose(const Fr& a, const Fr& b) { return {a.p + Rot(a.q, b.p), Norm(Mul(a.q, b.q))}; }
Fr Inverse(const Fr& a) {
    const Q c = Conj(a.q);
    return {Rot(c, a.p * -1.0), c};
}
V3 Apply(const Fr& a, V3 v) { return a.p + Rot(a.q, v); }

double Smooth(double t) {
    t = std::clamp(t, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// ---------------------------------------------------------------- the hand

struct Bone {
    int id = 0, parent = -1;
    Fr local;
    std::string name;
};

struct Rig {
    std::map<int, Bone> bones;
    int slot = 26;
};

// Each finger's bones: metacarpal (In*), then the three phalanges; the thumb has two phalanges after its metacarpal.
struct FingerDef {
    const char* name;
    int b[4];
    double r[3];
};
const FingerDef kFingerDefs[5] = {
    {"thumb", {33, 45, 55, -1}, {0.012, 0.0105, 0.0095}},
    {"index", {34, 46, 56, 65}, {0.0095, 0.0085, 0.0075}},
    {"middle", {35, 47, 57, 66}, {0.0095, 0.0085, 0.0075}},
    {"ring", {36, 48, 58, 67}, {0.0095, 0.0085, 0.0075}},
    {"pinky", {37, 49, 59, 68}, {0.009, 0.008, 0.007}},
};
constexpr int kThumb = 0, kIndex = 1, kMiddle = 2, kRing = 3, kPinky = 4;
constexpr double kPalmSkin = 0.014;

struct Capsule {
    V3 a, b;
    double r = 0;
    int finger = -1, link = 0;   // link: 0 proximal phalanx (the thumb's metacarpal), 1, 2 distal
};

struct Palm {
    V3 base, d, t, n;   // slot frame
    double halfD = 0, halfT = 0;
};

// Per bone, a flexion added to the rig's (the carry pose's) local rotation about the bone's own -Z.
using Flex = std::map<int, double>;

struct Hand {
    std::vector<Capsule> caps;
    Palm palm;
    std::map<int, Fr> world;   // slot frame
};

Fr LocalOf(const Rig& rig, int id, const Flex& flex) {
    Fr l = rig.bones.at(id).local;
    auto it = flex.find(id);
    if (it != flex.end() && it->second != 0.0) l.q = Norm(Mul(l.q, AxisAngle({0, 0, -1}, it->second)));
    return l;
}

// The flexion a local rotation carries about the bone's own -Z (its twist about Z, sign turned), radians.
double FlexOf(Q q) {
    double a = -2.0 * std::atan2(q.z, q.w);
    while (a > kPi) a -= 2 * kPi;
    while (a < -kPi) a += 2 * kPi;
    return a;
}

Fr WorldOf(const Rig& rig, int id, const Flex& flex, std::map<int, Fr>& memo) {
    auto it = memo.find(id);
    if (it != memo.end()) return it->second;
    const Bone& b = rig.bones.at(id);
    Fr w = LocalOf(rig, id, flex);
    if (b.parent >= 0 && rig.bones.count(b.parent)) w = Compose(WorldOf(rig, b.parent, flex, memo), w);
    memo[id] = w;
    return w;
}

Hand BuildHand(const Rig& rig, const Flex& flex) {
    std::map<int, Fr> hw;   // hand frame (bone 23)
    for (auto& [id, b] : rig.bones) WorldOf(rig, id, flex, hw);
    const Fr toSlot = Inverse(hw.at(rig.slot));
    Hand h;
    for (auto& [id, f] : hw) h.world[id] = Compose(toSlot, f);
    for (int f = 0; f < 5; ++f) {
        const FingerDef& d = kFingerDefs[f];
        const int last = d.b[3] >= 0 ? d.b[3] : d.b[2];
        std::vector<V3> J;
        for (int k = (f == kThumb ? 0 : 1); k < 4; ++k)
            if (d.b[k] >= 0) J.push_back(h.world.at(d.b[k]).p);
        // the fingertip: the distal bone's own reach again, three quarters of it (GraspPose.cpp TipOffset)
        J.push_back(Apply(h.world.at(last), rig.bones.at(last).local.p * 0.75));
        for (size_t s = 0; s + 1 < J.size(); ++s) h.caps.push_back({J[s], J[s + 1], d.r[s], f, int(s)});
    }
    // The palm (GraspPose.cpp MeasurePalm), measured in the hand frame and carried into the slot's.
    const V3 i1 = hw.at(46).p, m1 = hw.at(47).p, r1 = hw.at(48).p, p1 = hw.at(49).p;
    const V3 knuckle = (i1 + m1 + r1 + p1) * 0.25;
    V3 d = Unit(m1);
    const V3 across = i1 - p1;
    const double width = Len(across);
    V3 t = Unit(across - d * Dot(across, d));
    V3 n = Unit(Cross(d, t));
    {
        // the side the fingers close to: the middle finger's proximal phalanx turned about its own -Z
        const Fr mw = hw.at(47);
        const Fr next = rig.bones.at(57).local;
        const V3 a = Rot(mw.q, next.p);
        const V3 b = Rot(Mul(mw.q, AxisAngle({0, 0, -1}, 20 * kDeg)), next.p);
        if (Dot(b - a, n) < 0) n = n * -1.0;
    }
    const V3 base = knuckle * 0.5;
    h.palm.halfD = 0.5 * Dot(knuckle, d);
    h.palm.halfT = 0.5 * width + 0.008;
    h.palm.base = Apply(toSlot, base);
    h.palm.d = Rot(toSlot.q, d);
    h.palm.t = Rot(toSlot.q, t);
    h.palm.n = Rot(toSlot.q, n);
    return h;
}

V3 ToPalm(const Palm& pm, V3 p) {
    const V3 r = p - pm.base;
    return {Dot(r, pm.d), Dot(r, pm.t), Dot(r, pm.n)};
}
V3 FromPalm(const Palm& pm, V3 c) { return pm.base + pm.d * c.x + pm.t * c.y + pm.n * c.z; }

double PalmDist(const Palm& pm, V3 p) {
    const V3 rel = p - pm.base;
    const double q[3] = {std::fabs(Dot(rel, pm.d)) - pm.halfD, std::fabs(Dot(rel, pm.t)) - pm.halfT,
                         std::fabs(Dot(rel, pm.n)) - kPalmSkin};
    const double o[3] = {std::max(q[0], 0.0), std::max(q[1], 0.0), std::max(q[2], 0.0)};
    return std::sqrt(o[0] * o[0] + o[1] * o[1] + o[2] * o[2]) + std::min(std::max(q[0], std::max(q[1], q[2])), 0.0);
}

double CapsuleDist(const Capsule& c, V3 p) {
    const V3 ab = c.b - c.a;
    const double t = std::clamp(Dot(p - c.a, ab) / std::max(Dot(ab, ab), 1e-12), 0.0, 1.0);
    return Len(p - (c.a + ab * t)) - c.r;
}

// A finger's link into the palm (m, 0 when out), from `from` along it: a proximal phalanx starts inside the slab.
double CapsuleIntoPalm(const Palm& pm, const Capsule& c, double from) {
    double best = 1e9;
    for (int i = 0; i <= 12; ++i) best = std::min(best, PalmDist(pm, Lerp(c.a, c.b, from + (1 - from) * i / 12.0)) - c.r);
    return std::max(0.0, -best);
}

// ---------------------------------------------------------------- the shells

struct ShellDim {
    double r = 0.0104, len = 0.0507;
};

// A shell: a cylinder along its frame's +Z from 0 to len (the mesh's bind taken out: reload/shotgun.lua unbind).
double ShellDist(const ShellDim& s, const Fr& f, V3 p) {
    const V3 l = Rot(Conj(f.q), p - f.p);
    const double dr = std::sqrt(l.x * l.x + l.y * l.y) - s.r;
    const double dz = std::fabs(l.z - 0.5 * s.len) - 0.5 * s.len;
    const double ox = std::max(dr, 0.0), oz = std::max(dz, 0.0);
    return std::min(std::max(dr, dz), 0.0) + std::sqrt(ox * ox + oz * oz);
}

V3 ShellCentre(const ShellDim& s, const Fr& f) { return Apply(f, {0, 0, 0.5 * s.len}); }
Fr ShellAt(const ShellDim& s, V3 centre, Q q) { return {centre - Rot(q, {0, 0, 0.5 * s.len}), q}; }

// How deep a capsule goes into a shell (m, 0 when apart): its segment sampled, then refined around the deepest.
double CapsuleIntoShell(const ShellDim& s, const Fr& f, const Capsule& c) {
    double best = 1e9, bt = 0;
    const int N = 20;
    for (int i = 0; i <= N; ++i) {
        const double t = double(i) / N;
        const double d = ShellDist(s, f, Lerp(c.a, c.b, t)) - c.r;
        if (d < best) best = d, bt = t;
    }
    for (double h = 0.5 / N; h > 1e-4; h *= 0.5) {
        for (double t : {bt - h, bt + h}) {
            if (t < 0 || t > 1) continue;
            const double d = ShellDist(s, f, Lerp(c.a, c.b, t)) - c.r;
            if (d < best) best = d, bt = t;
        }
    }
    return std::max(0.0, -best);
}

// The palm's slab against a shell: points on the shell's axis and on its side facing the palm.
double PalmIntoShell(const ShellDim& s, const Fr& f, const Palm& pm) {
    double best = 1e9;
    const V3 axis = Rot(f.q, {0, 0, 1});
    V3 toward = pm.n * -1.0;
    toward = toward - axis * Dot(toward, axis);
    const bool side = Len(toward) > 1e-3;
    toward = Unit(toward);
    for (int i = 0; i <= 12; ++i) {
        const V3 c = Apply(f, {0, 0, s.len * i / 12.0});
        best = std::min(best, PalmDist(pm, c) - s.r);
        if (side) best = std::min(best, PalmDist(pm, c + toward * s.r));
    }
    return std::max(0.0, -best);
}

// Two shells: the gap between their surfaces (m, negative = overlap), one axis sampled against the other's field.
double ShellGap(const ShellDim& s, const Fr& a, const Fr& b) {
    double best = 1e9;
    for (int i = 0; i <= 16; ++i) best = std::min(best, ShellDist(s, b, Apply(a, {0, 0, s.len * i / 16.0})) - s.r);
    for (int i = 0; i <= 16; ++i) best = std::min(best, ShellDist(s, a, Apply(b, {0, 0, s.len * i / 16.0})) - s.r);
    return best;
}

// ---------------------------------------------------------------- input

struct Input {
    Rig rig;
    ShellDim shell;
    Fr carry;
    int count = 4;
    std::string gun;
};

bool Load(const char* path, Input& in) {
    std::ifstream f(path);
    if (!f) return false;
    std::string line;
    while (std::getline(f, line)) {
        std::istringstream s(line);
        std::string k;
        s >> k;
        if (k == "bone") {
            Bone b;
            s >> b.id >> b.parent >> b.local.p.x >> b.local.p.y >> b.local.p.z >> b.local.q.x >> b.local.q.y >>
                b.local.q.z >> b.local.q.w >> b.name;
            b.local.q = Norm(b.local.q);
            in.rig.bones[b.id] = b;
        } else if (k == "slot") {
            s >> in.rig.slot;
        } else if (k == "shell") {
            s >> in.shell.r >> in.shell.len;
        } else if (k == "carry") {
            s >> in.carry.p.x >> in.carry.p.y >> in.carry.p.z >> in.carry.q.x >> in.carry.q.y >> in.carry.q.z >>
                in.carry.q.w;
            in.carry.q = Norm(in.carry.q);
        } else if (k == "count") {
            s >> in.count;
        } else if (k == "gun") {
            s >> in.gun;
        }
    }
    return in.rig.bones.count(23) && in.rig.bones.count(in.rig.slot);
}

// The three joints of a closing finger (knuckle, middle, end) and their limits, degrees.
int JointBone(int finger, int j) { return kFingerDefs[finger].b[1 + j]; }
const double kJointMax[3] = {88.0, 105.0, 80.0};

// Straight: every joint of the middle, ring and little fingers back to no flexion.
Flex Straight(const Rig& rig) {
    Flex f;
    for (int finger = kMiddle; finger <= kPinky; ++finger)
        for (int j = 0; j < 3; ++j) f[JointBone(finger, j)] = -FlexOf(rig.bones.at(JointBone(finger, j)).local.q);
    return f;
}

// ---------------------------------------------------------------- checks

struct Report {
    double pen = 0;
    std::string worst;
};

// The deepest anything goes into anything: fingers into shells, the palm into shells, shells into each other.
// `allow` forgives each finger as deep in shell 1 as the carry as recorded presses it in.
using Allow = std::array<double, 5>;
Report Check(const Input& in, const Hand& h, const std::vector<Fr>& shells, const Allow& allow) {
    Report r;
    auto note = [&](double p, const std::string& who) {
        if (p > r.pen) r.pen = p, r.worst = who;
    };
    for (size_t i = 0; i < shells.size(); ++i) {
        for (const Capsule& c : h.caps) {
            double p = CapsuleIntoShell(in.shell, shells[i], c);
            if (i == 0) p = std::max(0.0, p - allow[c.finger]);
            note(p, std::string(kFingerDefs[c.finger].name) + " in shell " + std::to_string(i + 1));
        }
        note(PalmIntoShell(in.shell, shells[i], h.palm), "palm in shell " + std::to_string(i + 1));
        for (size_t j = 0; j < i; ++j)
            note(-ShellGap(in.shell, shells[i], shells[j]),
                 "shell " + std::to_string(i + 1) + " in shell " + std::to_string(j + 1));
    }
    return r;
}

// ---------------------------------------------------------------- placing the handful

// Nelder-Mead in 3 dimensions.
double Minimize(const std::function<double(const double*)>& f, double* x, double step, int iters) {
    double s[4][3], v[4];
    for (int i = 0; i < 4; ++i) {
        for (int k = 0; k < 3; ++k) s[i][k] = x[k] + (i == k + 1 ? step : 0.0);
        v[i] = f(s[i]);
    }
    for (int it = 0; it < iters; ++it) {
        int order[4] = {0, 1, 2, 3};
        std::sort(order, order + 4, [&](int a, int b) { return v[a] < v[b]; });
        const int hi = order[3], lo = order[0];
        double c[3] = {0, 0, 0};
        for (int i = 0; i < 4; ++i)
            if (i != hi)
                for (int k = 0; k < 3; ++k) c[k] += s[i][k] / 3.0;
        auto at = [&](double a, double* o) {
            for (int k = 0; k < 3; ++k) o[k] = c[k] + a * (s[hi][k] - c[k]);
            return f(o);
        };
        double r[3], e[3];
        const double vr = at(-1.0, r);
        if (vr < v[lo]) {
            const double ve = at(-2.0, e);
            if (ve < vr) std::copy_n(e, 3, s[hi]), v[hi] = ve;
            else std::copy_n(r, 3, s[hi]), v[hi] = vr;
        } else if (vr < v[order[2]]) {
            std::copy_n(r, 3, s[hi]), v[hi] = vr;
        } else {
            const double vc = at(0.5, e);
            if (vc < v[hi]) {
                std::copy_n(e, 3, s[hi]), v[hi] = vc;
            } else {
                for (int i = 0; i < 4; ++i)
                    if (i != lo) {
                        for (int k = 0; k < 3; ++k) s[i][k] = s[lo][k] + 0.5 * (s[i][k] - s[lo][k]);
                        v[i] = f(s[i]);
                    }
            }
        }
    }
    int best = 0;
    for (int i = 1; i < 4; ++i)
        if (v[i] < v[best]) best = i;
    std::copy_n(s[best], 3, x);
    return v[best];
}

struct Layout {
    double deg = 0;   // the stored shells' turn from the carry about the palm's normal
    Q q;
    std::vector<Fr> stored;   // in the order placed: the first deepest, the last taken first
    double cost = 0, pen = 0;
};

// The grip's line in palm coordinates: across the palm about 18 mm short of the knuckles, toward the little finger
// far enough that the crimps reach past it (it closes over them, not onto their ends).
V3 GripLine(const Input& in, const Palm& pm) { return {pm.halfD - 0.018, -0.012, kPalmSkin + in.shell.r + 0.0005}; }

Layout PlaceStored(const Input& in, const Hand& carry, const Hand& straight, double deg) {
    const Palm& pm = carry.palm;
    Layout L;
    L.deg = deg;
    L.q = Norm(Mul(AxisAngle(pm.n, deg * kDeg), in.carry.q));
    const V3 C = GripLine(in, pm);
    std::vector<Capsule> caps;
    for (const Capsule& c : carry.caps)
        if (c.finger == kThumb || c.finger == kIndex) caps.push_back(c);
    for (const Capsule& c : straight.caps)
        if (c.finger >= kMiddle) caps.push_back(c);
    auto frameAt = [&](const double* x) { return ShellAt(in.shell, FromPalm(pm, {x[0], x[1], x[2]}), L.q); };
    auto hard = [&](const Fr& f) {
        double e = 0;
        for (const Capsule& c : caps) {
            const double p = CapsuleIntoShell(in.shell, f, c);
            e += p * p;
        }
        const double pp = PalmIntoShell(in.shell, f, pm);
        e += pp * pp;
        const double g1 = ShellGap(in.shell, f, in.carry) - 0.002;   // shell 1 keeps a finger's breadth of air
        if (g1 < 0) e += g1 * g1;
        for (const Fr& o : L.stored) {
            const double g = ShellGap(in.shell, f, o) - 0.0003;
            if (g < 0) e += g * g;
        }
        return e;
    };
    for (int k = 0; k + 1 < in.count; ++k) {
        auto cost = [&](const double* x) {
            // nothing ahead of the grip's line: the proximal phalanges rise in front of the handful
            const double dd = x[0] - C.x, dt = x[1] - C.y, dn = x[2] - C.z, ahead = std::max(0.0, dd - 0.002);
            return 4e5 * (hard(frameAt(x)) + ahead * ahead) + 1.0 * dd * dd + 1.2 * dn * dn + 2.0 * dt * dt;
        };
        std::vector<V3> starts;
        if (L.stored.empty()) {
            for (double a : {0.0, 0.006, -0.006})
                for (double b : {0.0, 0.006}) starts.push_back({C.x + a, C.y, C.z + b});
        } else {
            for (const Fr& o : L.stored) {
                const V3 oc = ToPalm(pm, ShellCentre(in.shell, o));
                for (int a = 0; a < 12; ++a) {
                    const double th = a * 30.0 * kDeg;
                    starts.push_back({oc.x + 2.1 * in.shell.r * std::cos(th), oc.y,
                                      oc.z + 2.1 * in.shell.r * std::sin(th)});
                }
            }
        }
        double bestE = 1e30, best[3] = {C.x, C.y, C.z};
        for (const V3& s0 : starts) {
            double x[3] = {s0.x, s0.y, s0.z};
            double e = Minimize(cost, x, 0.004, 240);
            e = Minimize(cost, x, 0.001, 200);
            if (e < bestE) bestE = e, std::copy_n(x, 3, best);
        }
        const Fr f = frameAt(best);
        L.pen = std::max(L.pen, std::sqrt(hard(f)));
        L.cost += bestE;
        L.stored.push_back(f);
    }
    return L;
}

// ---------------------------------------------------------------- the fist on the shells

struct FistPose {
    double deg[3][3] = {};   // middle, ring, little: knuckle, middle, end joint flexion (absolute, degrees)
};

Flex FistFlex(const Rig& rig, const FistPose& p) {
    Flex f;
    for (int w = 0; w < 3; ++w)
        for (int j = 0; j < 3; ++j) {
            const int b = JointBone(kMiddle + w, j);
            f[b] = p.deg[w][j] * kDeg - FlexOf(rig.bones.at(b).local.q);
        }
    return f;
}

FistPose CarryFist(const Rig& rig) {
    FistPose p;
    for (int w = 0; w < 3; ++w)
        for (int j = 0; j < 3; ++j) p.deg[w][j] = FlexOf(rig.bones.at(JointBone(kMiddle + w, j)).local.q) / kDeg;
    return p;
}

// The middle, ring and little fingers closed from straight on `held` (and kept out of shell 1 and the palm): every
// joint closing together, the knuckle a little slower; a link that touches stops its own joint and the ones before.
FistPose Close(const Input& in, const std::vector<Fr>& held) {
    FistPose p;
    for (int w = 0; w < 3; ++w) {
        const int finger = kMiddle + w;
        double th[3] = {0, 0, 0};
        bool frozen[3] = {false, false, false};
        const double rate[3] = {0.8, 1.0, 0.8};
        auto touching = [&](const double* a) {
            FistPose t = p;
            for (int j = 0; j < 3; ++j) t.deg[w][j] = a[j];
            const Hand h = BuildHand(in.rig, FistFlex(in.rig, t));
            int top = -1;
            for (const Capsule& c : h.caps) {
                if (c.finger != finger) continue;
                const double p1 = CapsuleIntoShell(in.shell, in.carry, c);
                double ps = 0;
                for (const Fr& s : held) ps = std::max(ps, CapsuleIntoShell(in.shell, s, c));
                // the proximal phalanx starts inside the slab and only ever rises out of it
                const double pp = c.link == 0 ? 0.0 : CapsuleIntoPalm(h.palm, c, 0.0);
                const double pen = std::max(p1, std::max(ps, pp));
                if (pen > 0.0004) {
                    top = std::max(top, c.link);
                    if (gTrace)
                        std::printf("      %s link %d at %.0f/%.0f/%.0f: shell1 %.1f held %.1f palm %.1f mm\n",
                                    kFingerDefs[finger].name, c.link, a[0], a[1], a[2], p1 * 1e3, ps * 1e3, pp * 1e3);
                }
            }
            return top;
        };
        for (int it = 0; it < 600; ++it) {
            double prev[3] = {th[0], th[1], th[2]};
            bool moved = false;
            for (int j = 0; j < 3; ++j) {
                if (frozen[j]) continue;
                th[j] += rate[j] * 0.5;
                if (th[j] >= kJointMax[j]) th[j] = kJointMax[j], frozen[j] = true;
                moved = true;
            }
            if (!moved) break;
            const int top = touching(th);
            if (top >= 0)
                for (int j = 0; j <= top; ++j) th[j] = prev[j], frozen[j] = true;
        }
        for (int j = 0; j < 3; ++j) p.deg[w][j] = th[j];
    }
    return p;
}

// ---------------------------------------------------------------- the move into the fingers

struct Plan {
    double lift = 0, slide = 0, openIndex = 0, openThumb = 0;
    double ease[3] = {0, 0, 0};   // how far the middle, ring and little fingers let go (0 none, 1 straight)
    int early = 0;                // 1: the fist lets go at the start and has closed again by 0.6
    double score = 1e9, pen = 0;
    std::string worst;
};

struct Key {
    double t = 0;
    Fr shell;   // the one moving into the fingers
    Flex flex;
};

// The fingers at s of the move from c shells (fist `a` on c - 1 of them) to c - 1 (fist `b`).
Flex MoveFlex(const Input& in, const FistPose& a, const FistPose& b, const Plan& pl, double s) {
    const double bump = std::sin(kPi * std::clamp(s, 0.0, 1.0));
    const double give = pl.early ? std::sin(kPi * std::clamp(s / 0.6, 0.0, 1.0)) : bump;
    const double close = pl.early ? Smooth((s - 0.3) / 0.6) : Smooth((s - 0.35) / 0.65);
    FistPose f;
    for (int w = 0; w < 3; ++w)
        for (int j = 0; j < 3; ++j) {
            const double base = a.deg[w][j] + (b.deg[w][j] - a.deg[w][j]) * close;
            f.deg[w][j] = base * (1.0 - pl.ease[w] * give);   // the fist eases off toward straight
        }
    Flex flex = FistFlex(in.rig, f);
    for (int bone : {46, 56, 65}) flex[bone] = -pl.openIndex * kDeg * bump;
    for (int bone : {45, 55}) flex[bone] = -pl.openThumb * kDeg * bump;
    return flex;
}

Fr MoveShell(const Input& in, const Palm& pm, const Fr& from, const Plan& pl, double s) {
    const V3 c0 = ShellCentre(in.shell, from), c1 = ShellCentre(in.shell, in.carry);
    const V3 up = pm.n;
    // out of the fist first -- up off the palm, and drawn out by its brass (the thumb's end) -- then over and down
    // into the fingers: a cubic, both handles lifted
    const V3 brass = Rot(from.q, {0, 0, -1});
    const V3 P1 = c0 + up * pl.lift + brass * pl.slide, P2 = c1 + up * (0.6 * pl.lift);
    const double u = Smooth(s), v = 1 - u;
    const V3 c = c0 * (v * v * v) + P1 * (3 * v * v * u) + P2 * (3 * v * u * u) + c1 * (u * u * u);
    const Q q = Slerp(from.q, in.carry.q, Smooth((s - 0.15) / 0.7));
    return ShellAt(in.shell, c, q);
}

// How bad a plan is: the deepest anything goes into anything over the move (the carry's own pressing into shell 1
// forgiven as the shell arrives), plus what the hand has to do for it.
void Score(const Input& in, const Palm& pm, const Fr& taken, const std::vector<Fr>& rest, const FistPose& a,
           const FistPose& b, const Allow& allow, Plan& pl) {
    const int K = 13;
    double worst = 0;
    std::string who;
    for (int i = 1; i < K; ++i) {
        const double s = double(i) / (K - 1);
        const Hand h = BuildHand(in.rig, MoveFlex(in, a, b, pl, s));
        std::vector<Fr> shells{MoveShell(in, pm, taken, pl, s)};
        shells.insert(shells.end(), rest.begin(), rest.end());
        Allow al = allow;
        for (double& x : al) x *= Smooth((s - 0.45) / 0.45);
        const Report r = Check(in, h, shells, al);
        if (r.pen > worst) worst = r.pen, who = r.worst;
    }
    pl.pen = worst;
    pl.worst = who;
    pl.score = worst + 0.02 * pl.lift + 0.01 * pl.slide + 0.00002 * (pl.openIndex + std::fabs(pl.openThumb)) +
               0.0015 * (pl.ease[0] + pl.ease[1] + pl.ease[2]);
}

Plan PlanMove(const Input& in, const Palm& pm, const Fr& taken, const std::vector<Fr>& rest, const FistPose& a,
              const FistPose& b, const Allow& allow) {
    Plan best;
    auto score = [&](Plan& pl) { Score(in, pm, taken, rest, a, b, allow, pl); };
    // coarse: the fist easing evenly
    for (int early : {0, 1})
        for (double lift : {0.0, 0.01, 0.02, 0.03, 0.045})
            for (double slide : {0.0, 0.01, 0.02, 0.035})
                for (double oi : {0.0, 15.0, 30.0, 45.0})
                    for (double ot : {-30.0, -15.0, 0.0, 15.0, 30.0})
                        for (double ease : {0.0, 0.2, 0.4, 0.6}) {
                            Plan pl;
                            pl.lift = lift, pl.slide = slide, pl.openIndex = oi, pl.openThumb = ot, pl.early = early;
                            pl.ease[0] = pl.ease[1] = pl.ease[2] = ease;
                            score(pl);
                            if (pl.score < best.score) best = pl;
                        }
    // fine: one thing at a time, each finger's ease its own
    for (int pass = 0; pass < 4; ++pass) {
        bool better = false;
        auto tryOne = [&](double Plan::*field, double step, double lo, double hi) {
            for (double sgn : {-1.0, 1.0}) {
                Plan pl = best;
                pl.*field = std::clamp(pl.*field + sgn * step, lo, hi);
                score(pl);
                if (pl.score < best.score - 1e-7) best = pl, better = true;
            }
        };
        tryOne(&Plan::lift, 0.005, 0.0, 0.06);
        tryOne(&Plan::slide, 0.005, 0.0, 0.05);
        tryOne(&Plan::openIndex, 5.0, 0.0, 60.0);
        tryOne(&Plan::openThumb, 5.0, -40.0, 40.0);
        for (int w = 0; w < 3; ++w)
            for (double sgn : {-1.0, 1.0}) {
                Plan pl = best;
                pl.ease[w] = std::clamp(pl.ease[w] + sgn * 0.1, 0.0, 0.8);
                score(pl);
                if (pl.score < best.score - 1e-7) best = pl, better = true;
            }
        if (!better) break;
    }
    return best;
}

// ---------------------------------------------------------------- output

void LuaFrame(std::FILE* f, const Fr& x) {
    std::fprintf(f, "{ %.5f, %.5f, %.5f, %.5f, %.5f, %.5f, %.5f }", x.p.x, x.p.y, x.p.z, x.q.x, x.q.y, x.q.z, x.q.w);
}

// The finger pose a flexion map makes of the carry pose: every bone the pose sets, parent-local, left-hand names.
void LuaPose(std::FILE* f, const Rig& rig, const Flex& flex, const char* indent) {
    std::fprintf(f, "{\n");
    for (auto& [id, b] : rig.bones) {
        if (id == 23 || id == rig.slot) continue;
        const Q q = LocalOf(rig, id, flex).q;
        std::fprintf(f, "%s  { '%s', %.6f, %.6f, %.6f, %.6f },\n", indent, b.name.c_str(), q.x, q.y, q.z, q.w);
    }
    std::fprintf(f, "%s}", indent);
}

// ---------------------------------------------------------------- pictures

struct Scene {
    const Hand* hand;
    const ShellDim* dim;
    std::vector<Fr> shells;
    // the material of the nearest thing: 0 skin, 1 palm, 2 shell in the fingers, 3 other shell
    double Dist(V3 p, int* mat) const {
        double best = 1e9;
        int m = 0;
        for (const Capsule& c : hand->caps) {
            const double d = CapsuleDist(c, p);
            if (d < best) best = d, m = 0;
        }
        const double pd = PalmDist(hand->palm, p);
        if (pd < best) best = pd, m = 1;
        for (size_t i = 0; i < shells.size(); ++i) {
            const double d = ShellDist(*dim, shells[i], p);
            if (d < best) best = d, m = i == 0 ? 2 : 3;
        }
        if (mat) *mat = m;
        return best;
    }
};

void Render(const Scene& sc, V3 centre, const std::vector<std::pair<V3, V3>>& views, const std::string& path) {
    const int W = 320, H = 320;
    const double half = 0.08;
    std::vector<unsigned char> img(size_t(W) * views.size() * H * 3, 0);
    const size_t stride = size_t(W) * views.size() * 3;
    for (size_t v = 0; v < views.size(); ++v) {
        const V3 dir = Unit(views[v].first);
        const V3 right = Unit(Cross(dir, views[v].second));
        const V3 up = Cross(right, dir);
        const V3 light = Unit(dir * -1.0 + up * 0.6 + right * 0.3);
        for (int y = 0; y < H; ++y)
            for (int x = 0; x < W; ++x) {
                const double u = (2.0 * x / W - 1.0) * half, w = (1.0 - 2.0 * y / H) * half;
                V3 p = centre + right * u + up * w - dir * 0.25;
                double t = 0;
                int mat = -1;
                bool hit = false;
                for (int it = 0; it < 160 && t < 0.5; ++it) {
                    const double d = sc.Dist(p + dir * t, &mat);
                    if (d < 0.0002) {
                        hit = true;
                        break;
                    }
                    t += d;
                }
                double col[3] = {0.13, 0.14, 0.17};
                if (hit) {
                    const V3 q = p + dir * t;
                    const double e = 0.0004;
                    const V3 n = Unit({sc.Dist(q + V3{e, 0, 0}, nullptr) - sc.Dist(q - V3{e, 0, 0}, nullptr),
                                       sc.Dist(q + V3{0, e, 0}, nullptr) - sc.Dist(q - V3{0, e, 0}, nullptr),
                                       sc.Dist(q + V3{0, 0, e}, nullptr) - sc.Dist(q - V3{0, 0, e}, nullptr)});
                    const double l = 0.25 + 0.75 * std::max(0.0, Dot(n, light));
                    static const double kMat[4][3] = {{0.86, 0.70, 0.58}, {0.74, 0.58, 0.48}, {0.95, 0.55, 0.12},
                                                      {0.72, 0.12, 0.12}};
                    for (int k = 0; k < 3; ++k) col[k] = kMat[mat][k] * l;
                }
                unsigned char* o = &img[size_t(y) * stride + (v * W + x) * 3];
                for (int k = 0; k < 3; ++k) o[k] = (unsigned char)std::clamp(col[k] * 255.0, 0.0, 255.0);
            }
    }
    std::FILE* f = std::fopen(path.c_str(), "wb");
    if (!f) return;
    std::fprintf(f, "P6\n%d %d\n255\n", int(W * views.size()), H);
    std::fwrite(img.data(), 1, img.size(), f);
    std::fclose(f);
}

}  // namespace

int main(int argc, char** argv) {
    if (argc < 3) {
        std::fprintf(stderr, "handful.exe <in.txt> <out prefix> [--dump]\n");
        return 2;
    }
    Input in;
    if (!Load(argv[1], in)) {
        std::fprintf(stderr, "bad input %s\n", argv[1]);
        return 1;
    }
    const std::string out = argv[2];
    for (int i = 3; i < argc; ++i)
        if (std::strcmp(argv[i], "--trace") == 0) gTrace = true;
    const Hand base = BuildHand(in.rig, {});
    const Palm& pm = base.palm;
    if (argc > 3 && std::strcmp(argv[3], "--dump") == 0) {
        // palm coordinates: along the fingers d, across t (index side +), into the grip n
        const V3 ax = Rot(in.carry.q, {0, 0, 1});
        const V3 c0 = ToPalm(pm, in.carry.p), c1 = ToPalm(pm, Apply(in.carry, {0, 0, in.shell.len}));
        std::printf("palm halfD %.1f halfT %.1f skin %.1f mm\n", pm.halfD * 1e3, pm.halfT * 1e3, kPalmSkin * 1e3);
        std::printf("shell axis d %.2f t %.2f n %.2f; base (%.1f %.1f %.1f) top (%.1f %.1f %.1f) mm\n", Dot(ax, pm.d),
                    Dot(ax, pm.t), Dot(ax, pm.n), c0.x * 1e3, c0.y * 1e3, c0.z * 1e3, c1.x * 1e3, c1.y * 1e3,
                    c1.z * 1e3);
        for (const Capsule& c : base.caps) {
            const V3 a = ToPalm(pm, c.a), b = ToPalm(pm, c.b);
            std::printf("  %-6s r %.1f (%.1f %.1f %.1f) -> (%.1f %.1f %.1f)\n", kFingerDefs[c.finger].name, c.r * 1e3,
                        a.x * 1e3, a.y * 1e3, a.z * 1e3, b.x * 1e3, b.y * 1e3, b.z * 1e3);
        }
        return 0;
    }
    std::printf("%s: %d shells, r %.1f mm, len %.1f mm\n", in.gun.c_str(), in.count, in.shell.r * 1e3,
                in.shell.len * 1e3);
    Allow allow{};
    for (const Capsule& c : base.caps)
        allow[c.finger] = std::max(allow[c.finger], CapsuleIntoShell(in.shell, in.carry, c) + 0.0003);
    std::printf("  carry as recorded, into shell 1 (forgiven there): thumb %.1f index %.1f middle %.1f mm\n",
                allow[kThumb] * 1e3 - 0.3, allow[kIndex] * 1e3 - 0.3, allow[kMiddle] * 1e3 - 0.3);

    // 2. the stored ones, the turn about the palm's normal tried a few ways
    const Hand straight = BuildHand(in.rig, Straight(in.rig));
    Layout L;
    L.cost = 1e30;
    // the power grip's slant: brass by the index knuckle, the crimps out by the heel of the hand
    for (double deg : {-90.0, -105.0, -120.0}) {
        Layout t = PlaceStored(in, base, straight, deg);
        std::printf("  turn %+.0f: cost %.4g, deepest %.2f mm\n", deg, t.cost, t.pen * 1e3);
        if (t.cost < L.cost) L = t;
    }
    std::printf("  stored at turn %+.0f deg:\n", L.deg);
    for (size_t k = 0; k < L.stored.size(); ++k) {
        const V3 c = ToPalm(pm, ShellCentre(in.shell, L.stored[k]));
        std::printf("    %zu: centre d %.1f t %.1f n %.1f mm\n", k + 1, c.x * 1e3, c.y * 1e3, c.z * 1e3);
    }

    // 3. per count: shell 1, then the stored ones in the order they are taken (the last placed first)
    std::vector<std::vector<Fr>> hold(in.count + 1);
    std::vector<FistPose> fist(in.count + 1);
    std::vector<Flex> pose(in.count + 1);
    for (int c = 1; c <= in.count; ++c) {
        hold[c].push_back(in.carry);
        for (int k = c - 2; k >= 0; --k) hold[c].push_back(L.stored[k]);
        fist[c] = c == 1 ? CarryFist(in.rig) : Close(in, std::vector<Fr>(hold[c].begin() + 1, hold[c].end()));
        pose[c] = c == 1 ? Flex{} : FistFlex(in.rig, fist[c]);
        const Hand h = BuildHand(in.rig, pose[c]);
        const Report r = Check(in, h, hold[c], allow);
        std::printf("  %d in the hand: fist", c);
        for (int w = 0; w < 3; ++w)
            std::printf(" %s %.0f/%.0f/%.0f", kFingerDefs[kMiddle + w].name, fist[c].deg[w][0], fist[c].deg[w][1],
                        fist[c].deg[w][2]);
        std::printf("; deepest %.1f mm (%s)\n", r.pen * 1e3, r.worst.c_str());
    }

    // 4. the moves
    const int K = 17;
    std::vector<std::vector<Key>> moves(in.count + 1);
    for (int c = in.count; c >= 2; --c) {
        const Fr taken = hold[c][1];
        const std::vector<Fr> rest(hold[c].begin() + 2, hold[c].end());
        const Plan pl = PlanMove(in, pm, taken, rest, fist[c], fist[c - 1], allow);
        std::printf("  move %d -> %d: lift %.0f mm, drawn %.0f mm, index opens %.0f, thumb %+.0f, fist eases "
                    "%.0f/%.0f/%.0f%% %s; deepest %.1f mm (%s)\n",
                    c, c - 1, pl.lift * 1e3, pl.slide * 1e3, pl.openIndex, pl.openThumb, pl.ease[0] * 100,
                    pl.ease[1] * 100, pl.ease[2] * 100, pl.early ? "early" : "midway", pl.pen * 1e3,
                    pl.worst.c_str());
        for (int i = 0; i < K; ++i) {
            Key k;
            k.t = double(i) / (K - 1);
            k.shell = MoveShell(in, pm, taken, pl, k.t);
            k.flex = MoveFlex(in, fist[c], fist[c - 1], pl, k.t);
            if (i == 0) k.flex = pose[c];
            if (i == K - 1) k.flex = pose[c - 1], k.shell = in.carry;
            moves[c].push_back(k);
        }
    }

    // the data
    std::FILE* f = std::fopen((out + ".lua").c_str(), "w");
    if (!f) return 1;
    std::fprintf(f,
                 "-- GENERATED by tools/shell_handful (handful.cpp): the %s's handful of %d shells in the left hand.\n"
                 "-- Do not edit; run tools/shell_handful/run.py. Frames are hold frames in the weapon slot (x y z, "
                 "qx qy qz qw);\n-- poses are parent-local, left-hand names, like reload/poses.\n"
                 "--   hold[c][i]   shell i of c in the hand (1 = the one in the fingers = the carry, then the order "
                 "they are taken in)\n"
                 "--   pose[c]      the fingers with c in the hand (pose[1] = the carry pose)\n"
                 "--   move[c]      keys at t 0..1 of the next shell going into the fingers once shell 1 of c is in "
                 "the gun:\n"
                 "--                its frame and the fingers; the others stay at hold[c - 1][2..]\n"
                 "return {\n  radius = %.5f, length = %.5f, count = %d, moveTime = 0.42,\n  hold = {\n",
                 in.gun.c_str(), in.count, in.shell.r, in.shell.len, in.count);
    for (int c = 1; c <= in.count; ++c) {
        std::fprintf(f, "    [%d] = { ", c);
        for (const Fr& s : hold[c]) {
            LuaFrame(f, s);
            std::fprintf(f, ", ");
        }
        std::fprintf(f, "},\n");
    }
    std::fprintf(f, "  },\n  pose = {\n");
    for (int c = 1; c <= in.count; ++c) {
        std::fprintf(f, "    [%d] = ", c);
        LuaPose(f, in.rig, pose[c], "    ");
        std::fprintf(f, ",\n");
    }
    std::fprintf(f, "  },\n  move = {\n");
    for (int c = in.count; c >= 2; --c) {
        std::fprintf(f, "    [%d] = {\n", c);
        for (const Key& k : moves[c]) {
            std::fprintf(f, "      { t = %.4f, shell = ", k.t);
            LuaFrame(f, k.shell);
            std::fprintf(f, ",\n        pose = ");
            LuaPose(f, in.rig, k.flex, "        ");
            std::fprintf(f, " },\n");
        }
        std::fprintf(f, "    },\n");
    }
    std::fprintf(f, "  },\n}\n");
    std::fclose(f);

    // the pictures: from the palm's side, the thumb's, the fingertips', and three quarters into the palm
    const V3 centre = FromPalm(pm, {0.035, 0.0, 0.03});
    const std::vector<std::pair<V3, V3>> views = {
        {pm.n * -1.0, pm.d}, {pm.t * -1.0, pm.n}, {pm.d * -1.0, pm.n}, {pm.t, pm.n}, {pm.n, pm.d}};
    for (int c = 1; c <= in.count; ++c) {
        const Hand h = BuildHand(in.rig, pose[c]);
        Render(Scene{&h, &in.shell, hold[c]}, centre, views, out + "_hold" + std::to_string(c) + ".ppm");
    }
    for (int c = in.count; c >= 2; --c)
        for (int i : {4, 8, 12}) {
            const Key& k = moves[c][i];
            const Hand h = BuildHand(in.rig, k.flex);
            std::vector<Fr> shells{k.shell};
            shells.insert(shells.end(), hold[c].begin() + 2, hold[c].end());
            Render(Scene{&h, &in.shell, shells}, centre, views,
                   out + "_move" + std::to_string(c) + "_" + std::to_string(i) + ".ppm");
        }
    std::printf("wrote %s.lua and pictures\n", out.c_str());
    return 0;
}
