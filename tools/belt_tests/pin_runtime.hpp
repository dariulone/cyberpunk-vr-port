#pragma once
// grenade/pin_contact.lua and grenade.lua's pin timeline, replayed offline: the grenade's regrip, the thumb
// and finger keys over the hold they blend from, the ring's spring constraint and the distal snap after it
// lets go. Shared by bake_pin_contact.cpp (its checks) and pin_hand_report.cpp.
#include "pin_hand.hpp"
#include <optional>

namespace pin {
using hand::Pose;
constexpr double kDuration = 1.27;

inline double smooth(double u) {
    u = std::clamp(u, 0., 1.);
    return u * u * (3 - 2 * u);
}
// M.weight
inline double weight(double t) {
    return smooth(t / .18) * (1 - smooth((t - .95) / .32));
}
// M.snap: the distal phalanx straightening once the ring lets go, 0..1 over time since the release.
constexpr double kSnapRise = .025, kSnapHold = .07, kSnapDecay = .22;
inline double snap(double since) {
    if (since < 0)
        return 0;
    if (since < kSnapRise)
        return smooth(since / kSnapRise);
    if (since < kSnapRise + kSnapHold)
        return 1;
    return 1 - smooth((since - kSnapRise - kSnapHold) / kSnapDecay);
}

struct Key {
    double t = 0;
    Q q[3];
    Q grip;
    V shift;
};
// The four fingers' extra flexion over the hold at a time (degrees): index, middle, ring, little, each its MCP
// and PIP; the DIP follows the PIP at kDipShare.
struct FingerSample {
    double t = 0;
    std::array<double, 8> a{};
};
constexpr double kDipShare = .67;
inline void bendFinger(Pose &pose, const Pose &from, int chain, double mcp, double pip) {
    const int *c = hand::kChain[chain];
    pose[c[1]] = from[c[1]] * axis(2, -mcp);
    pose[c[2]] = from[c[2]] * axis(2, -pip);
    pose[c[3]] = from[c[3]] * axis(2, -pip * kDipShare);
}
struct Profile {
    V centre, normal, hinge, originalCentre, idleShift, tip;
    double radius = 0;
    Q pinRotation;
    Q rest[3], closedRest[3];
    T hold;
    std::vector<Key> keys;
    // One track per lever position, evenly spaced from open (0) to shut (1); each evenly spaced in time from
    // 0 to the end of the timeline.
    std::vector<std::vector<FingerSample>> fingers;
    bool snaps = false;
    Q snapQ[2]; // Thumb1, Thumb2 locals the snap turns toward
};

inline V vecOf(const Json &a) {
    return {a[0].get<double>(), a[1].get<double>(), a[2].get<double>()};
}
inline Q quatOf(const Json &a) {
    return {a[0].get<double>(), a[1].get<double>(), a[2].get<double>(), a[3].get<double>()};
}
inline Profile parse(const Json &j) {
    Profile p;
    p.centre = vecOf(j["centre"]);
    p.normal = unit(vecOf(j["normal"]));
    p.hinge = vecOf(j["hinge"]);
    p.originalCentre = vecOf(j["originalCentre"]);
    p.idleShift = vecOf(j["idleShift"]);
    p.tip = vecOf(j["tip"]);
    p.radius = j["radius"].get<double>();
    p.pinRotation = quatOf(j["pinRotation"]);
    for (int i = 0; i < 3; ++i) {
        p.rest[i] = quatOf(j["rest"][i]);
        p.closedRest[i] = quatOf(j["closedRest"][i]);
    }
    p.hold = {vecOf(j["hold"][0]), quatOf(j["hold"][1])};
    for (const auto &k : j["keys"]) {
        Key key;
        key.t = k["t"].get<double>();
        for (int i = 0; i < 3; ++i)
            key.q[i] = quatOf(k["q"][i]);
        key.grip = quatOf(k["grip"][0]);
        key.shift = vecOf(k["grip"][1]);
        p.keys.push_back(key);
    }
    if (j.contains("fingers"))
        for (const auto &rows : j["fingers"]) {
            std::vector<FingerSample> track;
            for (const auto &row : rows) {
                FingerSample f;
                f.t = row[0].get<double>();
                for (int i = 0; i < 8; ++i)
                    f.a[i] = row[i + 1].get<double>();
                track.push_back(f);
            }
            p.fingers.push_back(track);
        }
    if (j.contains("snap")) {
        p.snaps = true;
        p.snapQ[0] = quatOf(j["snap"][0]);
        p.snapQ[1] = quatOf(j["snap"][1]);
    }
    return p;
}

// M.sample
inline void sample(const Profile &p, double t, const Key *&a, const Key *&b, double &u) {
    for (size_t i = 1; i < p.keys.size(); ++i)
        if (t <= p.keys[i].t) {
            a = &p.keys[i - 1];
            b = &p.keys[i];
            u = smooth(std::max(0., (t - a->t) / (b->t - a->t)));
            return;
        }
    a = b = &p.keys.back();
    u = 0;
}

// The ring_flick additive the fingers carried before the contact keys had their own (the old profiles).
struct Flick {
    std::vector<std::pair<double, std::map<int, Q>>> keys;
    static Flick load(const Json &j) {
        Flick f;
        std::map<int, int> slotAt; // json index -> slot
        const std::pair<const char *, int> names[] = {{"LeftHandIndex1", 6}, {"LeftHandIndex2", 11},
                                                      {"LeftHandIndex3", 15}, {"LeftHandMiddle2", 12},
                                                      {"LeftHandRing2", 13}};
        for (auto [name, slot] : names)
            if (j["joints"].contains(name))
                slotAt[j["joints"][name].get<int>()] = slot;
        for (const auto &k : j["keys"]) {
            std::map<int, Q> q;
            for (auto [at, slot] : slotAt) {
                auto g = [&](int i) { return k[std::to_string(i)].get<double>(); };
                q[slot] = {g(at), g(at + 1), g(at + 2), g(at + 3)};
            }
            f.keys.push_back({k["1"].get<double>(), q});
        }
        return f;
    }
    void apply(double t, Pose &pose) const {
        if (keys.empty())
            return;
        size_t n = 1;
        while (n < keys.size() - 1 && t > keys[n].first)
            ++n;
        double u = t > keys.back().first ? 0 : smooth((t - keys[n - 1].first) / (keys[n].first - keys[n - 1].first));
        const auto &a = t > keys.back().first ? keys.back().second : keys[n - 1].second;
        const auto &b = t > keys.back().first ? keys.back().second : keys[n].second;
        for (auto &[slot, qa] : a) {
            Q q = blend(qa, b.at(slot), u);
            pose[slot] = pose[slot] * q;
        }
    }
};

// The hold a timeline starts from: the lever open (grenade_hold, the profile's hold) or shut (grenade_pinch,
// the closed hold).
struct Base {
    Pose fingers;
    Q thumb[3];
    T prop;
    double lever = 0; // 0 the lever open, 1 shut: which finger track it takes
};
// A hold part way between the two, as leverPose and holdNow blend them.
inline Base between(const Base &open, const Base &shut, double u) {
    Base b;
    for (int i = 0; i < hand::kSlots; ++i)
        b.fingers[i] = blend(open.fingers[i], shut.fingers[i], u);
    for (int i = 0; i < 3; ++i)
        b.thumb[i] = blend(open.thumb[i], shut.thumb[i], u);
    b.prop = {open.prop.p + (shut.prop.p - open.prop.p) * u, blend(open.prop.q, shut.prop.q, u)};
    b.lever = u;
    return b;
}

struct State {
    Pose pose;
    T prop;
};
// M.fingerAngles: the track read linearly between its samples.
inline std::array<double, 8> trackAt(const std::vector<FingerSample> &f, double t) {
    if (t <= f.front().t)
        return f.front().a;
    for (size_t i = 1; i < f.size(); ++i)
        if (t <= f[i].t) {
            double u = (t - f[i - 1].t) / (f[i].t - f[i - 1].t);
            std::array<double, 8> r;
            for (int k = 0; k < 8; ++k)
                r[k] = f[i - 1].a[k] + (f[i].a[k] - f[i - 1].a[k]) * u;
            return r;
        }
    return f.back().a;
}
// M.fingerAngles: piecewise linear in the lever between its tracks.
inline std::array<double, 8> fingersAt(const Profile &p, double t, double lever) {
    const size_t n = p.fingers.size();
    if (n == 1)
        return trackAt(p.fingers[0], t);
    double x = std::clamp(lever, 0., 1.) * double(n - 1);
    size_t i = std::min(n - 2, size_t(x));
    auto a = trackAt(p.fingers[i], t), b = trackAt(p.fingers[i + 1], t);
    double u = x - double(i);
    for (int k = 0; k < 8; ++k)
        a[k] += (b[k] - a[k]) * u;
    return a;
}
// The hand and the grenade at time t of the timeline, as grenadeHoldPose and poseApply set them.
inline State at(const Profile &p, const Base &base, double t, double releasedAt = -1, const Flick *flick = nullptr) {
    const Key *a, *b;
    double u;
    sample(p, t, a, b, u);
    double w = weight(t);
    State s;
    s.pose = base.fingers;
    for (int i = 0; i < 3; ++i)
        s.pose[hand::kChain[0][i]] = base.thumb[i];
    if (flick && p.fingers.empty())
        flick->apply(t, s.pose);
    for (int i = 0; i < 3; ++i) {
        Q target = p.rest[i] * blend(a->q[i], b->q[i], u);
        s.pose[hand::kChain[0][i]] = blend(base.thumb[i], target, w);
    }
    if (!p.fingers.empty()) {
        auto a8 = fingersAt(p, t, base.lever);
        for (int c = 1; c < 5; ++c)
            bendFinger(s.pose, s.pose, c, a8[(c - 1) * 2], a8[(c - 1) * 2 + 1]);
    }
    if (p.snaps && releasedAt >= 0) {
        double k = snap(t - releasedAt) * w;
        s.pose[5] = blend(s.pose[5], p.snapQ[0], k);
        s.pose[10] = blend(s.pose[10], p.snapQ[1], k);
    }
    Q gq = blend(a->grip, b->grip, u);
    V shift = a->shift + (b->shift - a->shift) * u;
    T target{p.hold.p + rot(p.hold.q, shift), p.hold.q * gq};
    s.prop = {base.prop.p + (target.p - base.prop.p) * w, blend(base.prop.q, target.q, w)};
    return s;
}

// M.step: the ring's spring on the thumb's distal segment (a -> b in the grenade's frame).
struct Contact {
    std::string phase = "approach";
    double force = 0, pull = 0;
    std::optional<double> along;
    V origin;
    V velocity;
};
inline double tipRadius(double along) {
    if (along >= .95)
        return .0062 - (along - .95) * .034;
    if (along >= .85)
        return .0076 - (along - .85) * .014;
    return .0085;
}
inline bool step(Contact &s, const Profile &p, V a, V b, double dt) {
    if (s.phase == "released")
        return false;
    V c = p.centre, n = p.normal, d = b - a;
    double den = dot(d, n);
    if (!s.along) {
        if (den > -.003)
            return false;
        double u = dot(c - a, n) / den;
        if (u < .65 || u > .985)
            return false;
        V x = a + d * u;
        double length = std::sqrt(dot(d, d));
        double radius = p.radius - tipRadius(u) * length / std::abs(den);
        if (radius < .0003)
            return false;
        if (dot(x - c, x - c) > radius * radius)
            return false;
        s.along = u;
        s.origin = x;
        s.phase = "hooked";
        return false;
    }
    V x = a + d * *s.along - s.origin;
    double pull = std::max(0., dot(x, n));
    double speed = (pull - s.pull) / std::max(.001, dt);
    V radialV = x - n * pull;
    double lim = p.radius - tipRadius(*s.along);
    if (dot(radialV, radialV) > lim * lim) {
        s = Contact{};
        return false;
    }
    s.force = std::max(0., 1500 * pull + 1.5 * speed);
    s.pull = pull;
    s.phase = pull > .0005 ? "tension" : "hooked";
    if (s.force >= 22 && pull >= .006) {
        s.phase = "released";
        double launch = std::min(3., std::max(1.6, std::sqrt(1500 / .04) * pull));
        s.velocity = n * launch;
        return true;
    }
    return false;
}

// The thumb's distal segment (Thumb2 to the profile's tip) in the grenade's frame.
inline void thumbSegment(const hand::Rig &rig, int side, const Profile &p, const State &s, V &a, V &b) {
    auto f = hand::fk(rig, side, s.pose);
    const T &d = f.bone[10];
    V A = d.p, B = d.p + rot(d.q, p.tip);
    a = rot(inv(s.prop.q), A - s.prop.p);
    b = rot(inv(s.prop.q), B - s.prop.p);
}

struct Run {
    double hookedAt = -1, releasedAt = -1, force = 0, pullAtRelease = 0, launch = 0;
    std::string lost; // when the hook let go before the release
    std::vector<std::pair<double, double>> pull; // time, the ring's travel along its normal
};
// The whole timeline at the game's frame rate; the release time it finds feeds the snap.
inline Run simulate(const hand::Rig &rig, int side, const Profile &p, const Base &base, double dt = 1. / 90,
                    const Flick *flick = nullptr) {
    Run r;
    Contact c;
    for (double t = 0; t <= kDuration; t += dt) {
        State s = at(p, base, t, -1, flick);
        V a, b;
        thumbSegment(rig, side, p, s, a, b);
        bool wasHooked = bool(c.along);
        bool released = step(c, p, a, b, dt);
        r.pull.push_back({t, c.along || released ? c.pull : 0.});
        if (released) {
            r.releasedAt = t;
            r.force = c.force;
            r.pullAtRelease = c.pull;
            r.launch = std::sqrt(dot(c.velocity, c.velocity));
            break;
        }
        if (c.along && r.hookedAt < 0)
            r.hookedAt = t;
        if (wasHooked && !c.along && r.lost.empty())
            r.lost = std::to_string(t);
    }
    return r;
}
} // namespace pin
