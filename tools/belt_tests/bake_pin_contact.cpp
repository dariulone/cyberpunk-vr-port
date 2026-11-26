// Solve the holding thumb, the grenade's regrip and the four fingers round it against each mesh opening.
// Inputs are the recorded neutral hand (fixtures/pin-contact), the rig and the game's first-person hand mesh
// (tools/grasp_tests), the hold poses and the WolvenKit JSON exports of the grenade meshes
// (build/pin-clearance/json). All pose generation runs here.
//
//   bake_pin_contact REPO output.lua
//
// THE THUMB IS A THUMB. Its metacarpal moves by elevation off the palm and angle out from the fingers with
// the roll that opposition brings (GraspPose.cpp's model, +-15 degrees of free play), the MCP flexes with a
// little abduction, the IP flexes; all inside a real thumb's range, with soft walls past its ease.
// THE FINGERS FOLLOW THE GRIP. When the grenade turns in the hand to bring the ring to the thumb, each finger's
// MCP and PIP (the DIP coupled) are solved against the real skin so it stays on the shell no deeper than the
// hold has it, opening a little (kOpenMost) rather than letting the grenade lie in an open palm.
// THE TIP STRAIGHTENS ON THE YANK. Once the ring lets go, Thumb2 snaps to a slight bend (the most the shell
// leaves room for) while Thumb1 keeps its bend, then both settle back.
#include "pin_runtime.hpp"
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdlib>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <nlohmann/json.hpp>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace fs = std::filesystem;
using hand::kDeg;

struct Model {
    T meta, prox, dist, prop;
};
// 0..5 the thumb's anatomical deltas from its rest (elevation, angle out, free roll, MCP flexion, MCP
// abduction, IP flexion; degrees), 6..8 the grip's turn (euler degrees), 9..11 its shift (mm).
using Params = std::array<double, 12>;
struct Pose {
    T meta, prox, dist, prop;
    V at, normal;
};
struct Opening {
    const char *name;
    V centre, normal;
    double radius;
};
const Opening openings[] = {{"frag", {-.00026, -.02865, .00737}, {-.0484, -.9982, .0359}, .01146},
                            {"ozob", {.00081, -.02866, .01000}, {-.0118, -.9955, .0940}, .01442},
                            {"flash", {-.00031, -.02741, .00639}, {.0048, -.9969, .0780}, .00877},
                            {"incendiary", {.00026, -.02947, -.00630}, {.0046, -.9909, .1342}, .00774},
                            {"biohazard", {.00025, -.02682, -.00186}, {.0064, -.9998, .0175}, .01015},
                            {"smoke", {.00062, -.02764, .00949}, {.0040, -.9997, .0222}, .01446}};
Json readJson(const fs::path &p) {
    std::ifstream f(p);
    if (!f)
        throw std::runtime_error("missing " + p.string());
    Json j;
    f >> j;
    return j;
}
Model load(const fs::path &path) {
    Json d = readJson(path);
    std::vector<T> b;
    for (auto &v : d["bones"])
        b.push_back({vec(v["p"]), quat(v["q"])});
    return {relative(b[0], b[1]), relative(b[1], b[2]), relative(b[2], b[3]),
            relative(b[0], compose(b[8], {vec(d["item"]["p"]), quat(d["item"]["q"])}))};
}
// The model's own thumb rotations.
Pose plain(const Model &m, double along) {
    Pose s;
    s.meta = m.meta;
    s.prox = compose(s.meta, m.prox);
    s.dist = compose(s.prox, m.dist);
    s.prop = m.prop;
    V tip = rot(s.dist.q, m.dist.p * .75);
    s.at = s.dist.p + tip * along;
    s.normal = unit(tip);
    return s;
}
T gripOf(const Model &m, const Params &p) {
    return {m.prop.p + rot(m.prop.q, {p[9] * .001, p[10] * .001, p[11] * .001}), m.prop.q * euler(p[6], p[7], p[8])};
}
Pose evaluate(const Model &m, const hand::Thumb &th, const Params &p, double along) {
    Q q[3];
    th.locals(p.data(), q);
    Model n = m;
    n.meta.q = q[0];
    n.prox.q = q[1];
    n.dist.q = q[2];
    n.prop = gripOf(m, p);
    return plain(n, along);
}
// The key's thumb rotations as the runtime stores them: deltas on the rest.
void deltas(const hand::Thumb &th, const Params &p, Q out[3]) {
    Q q[3];
    th.locals(p.data(), q);
    for (int i = 0; i < 3; ++i)
        out[i] = inv(th.rest[i]) * q[i];
}
// Between two keys at u, the whole of it weighted over a baseline, as pin_contact.lua blends them.
Pose between(const Model &m, const hand::Thumb &th, const Params &a, const Params &b, double u, double weight = 1,
             const Model *baseline = nullptr) {
    const Model &base = baseline ? *baseline : m;
    Q da[3], db[3];
    deltas(th, a, da);
    deltas(th, b, db);
    Model n = m;
    T *bones[3] = {&n.meta, &n.prox, &n.dist};
    const T *from[3] = {&base.meta, &base.prox, &base.dist};
    for (int i = 0; i < 3; ++i)
        bones[i]->q = blend(from[i]->q, th.rest[i] * blend(da[i], db[i], u), weight);
    Q prop = blend(euler(a[6], a[7], a[8]), euler(b[6], b[7], b[8]), u);
    V shift{a[9] + (b[9] - a[9]) * u, a[10] + (b[10] - a[10]) * u, a[11] + (b[11] - a[11]) * u};
    V target = m.prop.p + rot(m.prop.q, shift * .001);
    n.prop = {base.prop.p + (target - base.prop.p) * weight, blend(base.prop.q, m.prop.q * prop, weight)};
    return plain(n, .94);
}

// A finger rests this far off the shell.
constexpr double kMargin = .0008;
// THE FINGERS KEEP HOLD: a finger opens no more than kOpenMost at a joint over the hold (a hand that lets the
// grenade lie in an open palm is worse than a finger a millimetre into the shell for a moment), and past
// kOpenEase it costs. The index lifts on its own more readily than the three that close round the body:
// kOpenIndex for it.
const double kOpenMost = std::getenv("PIN_OPEN_MOST") ? std::atof(std::getenv("PIN_OPEN_MOST")) : 20;
const double kOpenIndex = std::getenv("PIN_OPEN_INDEX") ? std::atof(std::getenv("PIN_OPEN_INDEX")) : 30;
const double kOpenEase = std::getenv("PIN_OPEN_EASE") ? std::atof(std::getenv("PIN_OPEN_EASE")) : 10;
// A finger the hold has inside the shell lifts out of it by no more than kLift as the regrip comes in: the hold's
// own depth is the approved look, and lifting all the way out of a shell the hold sinks into opens the hand.
const double kLift = (std::getenv("PIN_FINGER_LIFT") ? std::atof(std::getenv("PIN_FINGER_LIFT")) : 0) / 1000;
// A finger that rests on the shell stays on it (kGapWeight per mm squared off it) and moves smoothly (kSmoothWeight
// per degree squared a sample).
const double kGapWeight = std::getenv("PIN_GAP_W") ? std::atof(std::getenv("PIN_GAP_W")) : 30;
const double kSmoothWeight = std::getenv("PIN_SMOOTH_W") ? std::atof(std::getenv("PIN_SMOOTH_W")) : .3;
// The regrip's cost per degree squared of turn and per millimetre squared of shift, and the most of each.
double tunable(const char *name, double fallback) {
    const char *v = std::getenv(name);
    return v ? std::atof(v) : fallback;
}
const double kTiltWeight = tunable("PIN_TILT_W", .02), kShiftWeight = tunable("PIN_SHIFT_W", .3);
const double kTiltMost = tunable("PIN_TILT_MAX", 60), kShiftMost = tunable("PIN_SHIFT_MAX", 25);
const double kFingerWeight = tunable("PIN_FINGER_W", .05);

struct Fit {
    Model m;
    Opening o;
    const hand::Thumb *th = nullptr;
    const hand::Rig *rig = nullptr;
    int side = 0;
    Params preferred{}, lo{}, hi{};
    double along = .94, travel = 0;
    // How hard the tip is held to its target against how hard the thumb is held to its ease.
    double targetWeight = 30, anatomyWeight = .5;
    // The tip pointing into the opening, and the metacarpal held where the preferred pose has it (x weight).
    double facingWeight = 2600, baseHold = 1;
    bool moveProp = true;
    const PinBody *body = nullptr;
    const std::vector<V> *palm = nullptr; // the hand's skin no finger moves, in the hand's frame
    double handedness = -1;
    const Params *pathFrom = nullptr;
    // Ways between keys checked as the runtime plays them, the regrip's weight and all, from both ends of the
    // lever (`holds`): each from key a at ta to key b at tb, nullptr meaning the key being solved.
    struct Segment {
        const Params *a, *b;
        double ta, tb;
    };
    std::vector<Segment> segments;
    const Model *holds[2] = {nullptr, nullptr};
    double palmLeast = 0; // the hold's own palm clearance: the regrip goes no deeper
    // Each finger's own skin at the hold and how deep the hold already has it: the regrip that turns the
    // shell into a finger costs what it would take that finger to open out of it.
    const std::vector<std::pair<int, V>> *fingerSkin = nullptr;
    std::array<double, 5> fingerFloor{};
    double fingerPenetration(const T &prop) const {
        if (!fingerSkin)
            return 0;
        double sum = 0;
        Q qi = inv(prop.q);
        for (const auto &[chain, v] : *fingerSkin) {
            double d = body->distance(rot(qi, v - prop.p));
            if (d < fingerFloor[chain]) {
                double e = (fingerFloor[chain] - d) * 1000;
                sum += e * e;
            }
        }
        return sum;
    }
    // The most the IP may bend at this key past which a wall rises (degrees; the whole range otherwise).
    double ipMost = 1e9;
    double anatomy(const Params &p) const {
        Q q[3];
        th->locals(p.data(), q);
        auto a = th->read(*rig, side, q);
        double over = std::max(0., a.ip - ipMost);
        return th->strain(a) + 4 * over * over;
    }
    double palmPenetration(const T &prop) const {
        if (!palm)
            return 0;
        double sum = 0, floor = std::min(.0005, palmLeast);
        Q qi = inv(prop.q);
        for (V v : *palm) {
            double d = body->distance(rot(qi, v - prop.p));
            if (d < floor) {
                double e = (floor - d) * 1000;
                sum += e * e;
            }
        }
        return sum;
    }
    double cost(const Params &p) const {
        auto s = evaluate(m, *th, p, along);
        V n = unit(rot(s.prop.q, o.normal)), target = s.prop.p + rot(s.prop.q, o.centre) + n * travel;
        V d = (s.at - target) * 1000;
        // The bent distal thumb sits beyond the loop. Retracting it pulls
        // outward from the shell, so the proximal segment must enter inward.
        double facing = 1 + dot(n, s.normal);
        double result = targetWeight * dot(d, d) + facingWeight * facing * facing;
        if (body) {
            double intrusion =
                std::max(0., .0006 - body->clearance(s.prop, s.meta, s.prox, s.dist, handedness)) * 1000;
            result += 300 * intrusion * intrusion;
            if (pathFrom)
                for (double u : {.1, .2, .3, .4, .5, .6, .7, .8, .9}) {
                    auto sample = between(m, *th, *pathFrom, p, u);
                    double penetration =
                        std::max(0., .002 - body->clearance(sample.prop, sample.meta, sample.prox,
                                                            sample.dist, handedness)) *
                        1000;
                    result += 300 * penetration * penetration;
                }
            if (moveProp)
                result += 3 * palmPenetration(s.prop) + kFingerWeight * fingerPenetration(s.prop);
            for (const auto &seg : segments)
                for (const Model *hold : holds)
                    for (int k = 1; k <= 7; ++k) {
                        const double f = k / 8., t = seg.ta + (seg.tb - seg.ta) * f;
                        auto sample = between(m, *th, seg.a ? *seg.a : p, seg.b ? *seg.b : p, pin::smooth(f),
                                              pin::weight(t), hold);
                        double penetration =
                            std::max(0., .002 - body->clearance(sample.prop, sample.meta, sample.prox, sample.dist,
                                                                handedness)) *
                            1000;
                        result += 300 * penetration * penetration;
                    }
        }
        result += anatomyWeight * anatomy(p);
        // Elevation, angle out, free roll, MCP flexion, MCP abduction, IP; the grip's turn, its shift.
        const double weights[12] = {.008, .008, .03, .01, .04, .006, kTiltWeight, kTiltWeight, kTiltWeight,
                                    kShiftWeight, kShiftWeight, kShiftWeight};
        for (int j = 0; j < 12; ++j) {
            double delta = p[j] - preferred[j];
            result += delta * delta * weights[j] * (j < 3 ? baseHold : 1);
        }
        return result;
    }
    Params solve(Params p) const {
        for (int j = 0; j < 12; ++j)
            p[j] = std::clamp(p[j], lo[j], hi[j]);
        for (double step : {12., 6., 3., 1.5, .6, .25, .1, .04, .015}) {
            for (int iteration = 0; iteration < 160; ++iteration) {
                double best = cost(p);
                bool improved = false;
                for (int j = 0; j < (moveProp ? 12 : 6); ++j)
                    for (double sign : {-1., 1.}) {
                        auto trial = p;
                        trial[j] = std::clamp(p[j] + sign * step, lo[j], hi[j]);
                        double c = cost(trial);
                        if (c < best) {
                            p = trial;
                            best = c;
                            improved = true;
                        }
                    }
                if (!improved)
                    break;
            }
        }
        return p;
    }
};

// ---------------------------------------------------------------- the four fingers

// THE FINGERS THROUGH THE REGRIP, a sample every kFingerStep: each finger's MCP and PIP over the hold, no
// deeper in the shell than the hold has it (lifting out by up to kLift once the regrip is in), resting on the
// shell where it rested in the hold, moving smoothly and home again at the end.
constexpr double kFingerStep = .02, kTouch = .0012;
// `neighbour`: the track already solved for the lever a step more open, and `between` the hold half way to it:
// the runtime blends the two for a lever between, so that blend is kept out of the shell too.
std::vector<pin::FingerSample> fingerTrack(const hand::Rig &rig, hand::HandMesh &M, const PinBody &body,
                                           const pin::Profile &prof, const pin::Base &base,
                                           const std::array<std::vector<int>, 5> &moving,
                                           const std::vector<pin::FingerSample> *neighbour = nullptr,
                                           const pin::Base *between = nullptr) {
    auto least = [&](const T &at, const hand::Pose &pose, int chain, double floor, double *pen,
                     double firm = -1, double *hard = nullptr) {
        M.skin(rig, pose, chain + 1);
        Q qi = inv(at.q);
        double l = 1e9, sum = 0, under = 0;
        for (int k : moving[chain]) {
            double d = body.distance(rot(qi, M.sv[k] - at.p));
            l = std::min(l, d);
            if (d < floor) {
                double e = (floor - d) * 1000;
                sum += e * e;
            }
            if (d < firm) {
                double e = (firm - d) * 1000;
                under += e * e;
            }
        }
        if (pen)
            *pen = sum;
        if (hard)
            *hard = under;
        return l;
    };
    std::array<double, 5> hold{}, holdBetween{};
    for (int c = 1; c < 5; ++c) {
        hold[c] = least(base.prop, base.fingers, c, -1, nullptr);
        if (between)
            holdBetween[c] = std::min(kMargin, least(between->prop, between->fingers, c, -1, nullptr));
    }
    // One sample: each finger from its neighbour's bend (`from`, at time `near`), the way between them
    // checked half way, as the runtime reads it.
    auto solveSample = [&](int index, double t, const std::array<double, 8> &from, double near) {
        auto s = pin::at(prof, base, t);
        const bool blendLever = neighbour && between;
        const pin::State sl = blendLever ? pin::at(prof, *between, t) : s;
        double w = pin::weight(t);
        pin::FingerSample f;
        f.t = t;
        // A quarter, a half and three quarters of the way back to the neighbour (u: the share of this sample).
        constexpr double kBetween[3] = {.25, .5, .75};
        pin::State sm[3];
        double tmid[3];
        for (int k = 0; k < 3; ++k) {
            tmid[k] = near + (t - near) * kBetween[k];
            sm[k] = pin::at(prof, base, tmid[k]);
        }
        for (int c = 1; c < 5; ++c) {
            // Never deeper than the hold has it (firm); lifting out of the shell by up to kLift as the regrip
            // comes in (soft).
            double own = std::min(kMargin, hold[c]);
            double firm = own;
            double floor = own + std::min(kMargin - own, kLift) * w;
            bool touches = hold[c] < .004;
            // Home as the regrip leaves: near either end the hold's own depth is allowed, so whatever is
            // left of the bend is slack and goes.
            double home = .002 + (1 - pin::smooth(w / .15));
            double pm = from[(c - 1) * 2], pp = from[(c - 1) * 2 + 1];
            const double openMost = c == 1 ? kOpenIndex : kOpenMost;
            auto cost = [&](double m, double p) {
                hand::Pose pose = s.pose;
                pin::bendFinger(pose, s.pose, c, m, p);
                double pen, hard;
                double l = least(s.prop, pose, c, floor, &pen, firm, &hard);
                for (int k = 0; k < 3; ++k) {
                    const double u = kBetween[k], floorMid = own + std::min(kMargin - own, kLift) * pin::weight(tmid[k]);
                    hand::Pose mid = sm[k].pose;
                    pin::bendFinger(mid, sm[k].pose, c, pm + (m - pm) * u, pp + (p - pp) * u);
                    double penMid, hardMid;
                    double lm = least(sm[k].prop, mid, c, floorMid, &penMid, firm, &hardMid);
                    double belowMid = std::max(0., (firm - lm) * 1000);
                    pen += .3 * penMid;
                    hard += hardMid + 4 * belowMid * belowMid;
                }
                if (blendLever) {
                    const auto &na = (*neighbour)[index].a;
                    hand::Pose pl = sl.pose;
                    pin::bendFinger(pl, sl.pose, c, (m + na[(c - 1) * 2]) * .5, (p + na[(c - 1) * 2 + 1]) * .5);
                    double penL, hardL;
                    double ll = least(sl.prop, pl, c, -1, &penL, holdBetween[c], &hardL);
                    double belowL = std::max(0., (holdBetween[c] - ll) * 1000);
                    hard += hardL + 4 * belowL * belowL;
                }
                double gap = touches ? std::max(0., (l - floor - kTouch) * 1000) : 0;
                double closing = std::max(0., m) * std::max(0., m) + std::max(0., p) * std::max(0., p);
                double deficit = std::max(0., (floor - l) * 1000), below = std::max(0., (firm - l) * 1000);
                // Opening past kOpenEase at a joint reads as letting go: it has to buy a lot of clearance.
                double wide = std::max(0., -m - kOpenEase) * std::max(0., -m - kOpenEase) +
                              std::max(0., -p - kOpenEase) * std::max(0., -p - kOpenEase);
                return 2 * pen + 30 * deficit * deficit + 200 * hard + 800 * below * below + kGapWeight * gap * gap +
                       .05 * wide + kSmoothWeight * ((m - pm) * (m - pm) + (p - pp) * (p - pp)) +
                       home * (m * m + p * p) + .05 * closing;
            };
            // From the neighbour, and from a little more open: the knuckle and the middle joint trade off, so
            // the steps go both ways at once as well as one joint at a time.
            double bm = pm, bp = pp, best = cost(bm, bp);
            for (auto [sm2, sp2] : {std::pair{-8., -8.}, std::pair{-16., -4.}, std::pair{-4., -16.}, std::pair{0., 0.}}) {
                double m = std::clamp(sm2 == 0 && sp2 == 0 ? 0. : pm + sm2, -openMost, 20.),
                       p = std::clamp(sm2 == 0 && sp2 == 0 ? 0. : pp + sp2, -openMost, 20.), v = cost(m, p);
                if (v < best)
                    best = v, bm = m, bp = p;
            }
            constexpr double moves[8][2] = {{1, 0}, {-1, 0}, {0, 1}, {0, -1}, {1, 1}, {-1, -1}, {1, -1}, {-1, 1}};
            for (double step : {8., 4., 2., 1., .5, .25})
                for (int it = 0; it < 60; ++it) {
                    bool moved = false;
                    for (const auto &mv : moves) {
                        double m = std::clamp(bm + mv[0] * step, -openMost, 20.);
                        double p = std::clamp(bp + mv[1] * step, -openMost, 20.);
                        double v = cost(m, p);
                        if (v < best)
                            best = v, bm = m, bp = p, moved = true;
                    }
                    if (!moved)
                        break;
                }
            if (std::getenv("PIN_DEBUG_TRACK")) {
                hand::Pose pose = s.pose;
                pin::bendFinger(pose, s.pose, c, bm, bp);
                double l = least(s.prop, pose, c, floor, nullptr);
                if (l < floor - .0002)
                    std::cout << "    track t " << t << " w " << w << " finger " << c << " floor " << floor * 1000
                              << " got " << l * 1000 << " at " << bm << "/" << bp << "\n";
            }
            f.a[(c - 1) * 2] = bm;
            f.a[(c - 1) * 2 + 1] = bp;
        }
        return f;
    };
    // OUT FROM THE HOLD AT BOTH ENDS: forward from the first frame to a seam in the window where the regrip is
    // fully in, backward from the last frame to it -- each half grows out of the hold's own pose, so neither has
    // to find its way back into it.
    const int count = int(std::ceil(pin::kDuration / kFingerStep - 1e-9));
    auto timeOf = [&](int i) { return std::min(pin::kDuration, i * kFingerStep); };
    auto build = [&](double seamAt) {
        const int middle = int(std::lround(seamAt / kFingerStep));
        std::vector<pin::FingerSample> track(count + 1);
        track[0].t = 0;
        for (int i = 1; i <= middle; ++i)
            track[i] = solveSample(i, timeOf(i), track[i - 1].a, timeOf(i - 1));
        track[count].t = timeOf(count);
        for (int i = count - 1; i > middle; --i)
            track[i] = solveSample(i, timeOf(i), track[i + 1].a, timeOf(i + 1));
        // The two halves may have found different bends at the seam (a finger curled on to the shell from the
        // front, opened off it from the back): across kSeam either side of it each sample is solved from the
        // straight way between the two, then twice more from the mean of its neighbours, so the change is spread
        // over a third of a second and every sample still checked.
        constexpr int kSeam = 8;
        const int seamFrom = std::max(1, middle - kSeam), seamTo = std::min(count - 1, middle + kSeam);
        if (seamTo > seamFrom) {
            const auto a = track[seamFrom - 1].a, b = track[seamTo + 1].a;
            for (int i = seamFrom; i <= seamTo; ++i) {
                const double u = double(i - seamFrom + 1) / (seamTo - seamFrom + 2);
                std::array<double, 8> way;
                for (int k = 0; k < 8; ++k)
                    way[k] = a[k] + (b[k] - a[k]) * u;
                track[i] = solveSample(i, timeOf(i), way, timeOf(i - 1));
            }
            for (int pass = 0; pass < 2; ++pass)
                for (int i = seamFrom; i <= seamTo; ++i) {
                    std::array<double, 8> mean;
                    for (int k = 0; k < 8; ++k)
                        mean[k] = (track[i - 1].a[k] + track[i + 1].a[k]) * .5;
                    track[i] = solveSample(i, timeOf(i), mean, timeOf(i - 1));
                }
        }
        // As the clip stores them (hundredths of a degree), so the checks see what the game reads.
        for (auto &f : track)
            for (double &x : f.a)
                x = std::round(x * 100) / 100;
        return track;
    };
    // How far past the hold's own depth each finger goes along a track: every sample, half way between samples,
    // and the blend with the neighbouring lever's track at the hold half way to it.
    auto deepest = [&](const std::vector<pin::FingerSample> &track) {
        std::array<double, 5> d{};
        auto at = [&](const pin::Base &b, double t, const std::array<double, 8> &x, const std::array<double, 5> &firm) {
            const auto s = pin::at(prof, b, t);
            for (int c = 1; c < 5; ++c) {
                hand::Pose pose = s.pose;
                pin::bendFinger(pose, s.pose, c, x[(c - 1) * 2], x[(c - 1) * 2 + 1]);
                d[c] = std::max(d[c], firm[c] - least(s.prop, pose, c, -1, nullptr));
            }
        };
        std::array<double, 5> own{};
        for (int c = 1; c < 5; ++c)
            own[c] = std::min(kMargin, hold[c]);
        for (int i = 0; i <= count; ++i) {
            at(base, track[i].t, track[i].a, own);
            if (i < count) {
                std::array<double, 8> mid;
                for (int k = 0; k < 8; ++k)
                    mid[k] = (track[i].a[k] + track[i + 1].a[k]) * .5;
                at(base, (track[i].t + track[i + 1].t) * .5, mid, own);
            }
            if (neighbour && between) {
                std::array<double, 8> blend;
                for (int k = 0; k < 8; ++k)
                    blend[k] = (track[i].a[k] + (*neighbour)[i].a[k]) * .5;
                at(*between, track[i].t, blend, holdBetween);
            }
        }
        return d;
    };
    const double seamDefault = std::getenv("PIN_SEAM") ? std::atof(std::getenv("PIN_SEAM")) : .6;
    auto track = build(seamDefault);
    auto depth = deepest(track);
    // A finger the default seam sinks past the hold: the seam early in the window and late in it as well, that
    // finger's track from whichever goes least deep (each finger is solved on its own, so they mix).
    if (*std::max_element(depth.begin() + 1, depth.end()) > .0006)
        for (double seamAt : {.3, .9}) {
            const auto other = build(seamAt);
            const auto otherDepth = deepest(other);
            for (int c = 1; c < 5; ++c)
                if (otherDepth[c] < depth[c] - .0001) {
                    depth[c] = otherDepth[c];
                    for (size_t i = 0; i < track.size(); ++i)
                        for (int k : {(c - 1) * 2, (c - 1) * 2 + 1})
                            track[i].a[k] = other[i].a[k];
                }
        }
    return track;
}

void printRow(std::ostream &out, const std::vector<double> &a) {
    out << "{";
    for (size_t i = 0; i < a.size(); ++i) {
        if (i)
            out << ",";
        out << a[i];
    }
    out << "}";
}
void printQ(std::ostream &out, Q q) {
    printRow(out, {q.x, q.y, q.z, q.w});
}

int main(int argc, char **argv) try {
    if (argc != 3 && argc != 4)
        throw std::runtime_error("REPO output.lua [--lenient]");
    // --lenient: write the clip with the trajectory's violations listed, for looking at them.
    const bool lenient = argc == 4 && std::string(argv[3]) == "--lenient";
    int violations = 0;
    const fs::path repo = argv[1];
    const fs::path fixtures = repo / "tools/belt_tests/fixtures/pin-contact", meshDir = repo / "build/pin-clearance/json";
    const fs::path poses = repo / "mods/cet/CyberpunkVRPort_TacticalBelt/grenade/poses";
    const auto rig = hand::Rig::load(repo / "tools/grasp_tests/rig.txt");
    const auto holdPose = hand::loadPose(poses / "grenade_hold.lua"), pinchPose = hand::loadPose(poses / "grenade_pinch.lua");
    hand::HandMesh meshes[2] = {hand::HandMesh::load(repo / "tools/grasp_tests/mesh/hand_l.bin", 0, rig),
                                hand::HandMesh::load(repo / "tools/grasp_tests/mesh/hand_r.bin", 1, rig)};
    const Json closed = readJson(fixtures / "closed.json");
    // Finish every solve before touching the checked-in clip.
    std::ostringstream out;
    out << std::fixed << std::setprecision(8)
        << "-- Generated by tools/belt_tests/bake_pin_contact.cpp from holding-hand FK, the rig and the\n"
           "-- first-person hand mesh. keys: t, q (thumb deltas on rest), grip (turn, shift); fingers: t, then\n"
           "-- index, middle, ring, little MCP/PIP flexion over the hold (degrees, DIP at 0.67 of the PIP);\n"
           "-- snap: Thumb1, Thumb2 locals after the release.\n"
           "return {\n";
    std::vector<PinBody> bodies;
    for (auto o : openings) {
        auto body = PinBody::load(meshDir, o.name);
        body.buildField();
        bodies.push_back(std::move(body));
    }
    std::cout << std::fixed << std::setprecision(2);
    for (int side = 0; side < 2; ++side) {
        Model m = load(fixtures / (side ? "right.json" : "left.json"));
        auto &mesh = meshes[side];
        hand::Pose restPose = holdPose;
        restPose[0] = m.meta.q;
        restPose[5] = m.prox.q;
        restPose[10] = m.dist.q;
        const auto th = hand::Thumb::make(rig, side, restPose);
        // The skin each finger moves, and the skin none does (the palm, the knuckles' heels).
        std::array<std::vector<int>, 5> moving;
        std::vector<int> still;
        std::vector<V> palm;
        mesh.skin(rig, restPose);
        const std::vector<V> holdSkin = mesh.sv;
        std::vector<std::pair<int, V>> fingerSkin;
        {
            auto parts = hand::partition(mesh);
            moving = parts.moving;
            still = parts.still;
            for (int c = 1; c < 5; ++c)
                for (int k : moving[c])
                    fingerSkin.push_back({c, mesh.sv[k]});
            for (int k : still)
                palm.push_back(mesh.sv[k]);
        }
        std::cout << "side " << side << ": rest thumb elev " << th.elev0 << " away " << th.away0 << " mcp "
                  << th.mcp0.flex << " (abd " << th.mcp0.abd << ") ip " << th.ip0.flex << "; palm skin "
                  << palm.size() << " vertices\n";
        Params lo, hi;
        lo = {hand::kElevHard[0] - th.elev0, hand::kAwayHard[0] - th.away0, -2 * hand::kRollFree,
              hand::kMcpHard[0] - th.mcp0.flex, hand::kAbdHard[0], hand::kIpHard[0] - th.ip0.flex, -kTiltMost, -kTiltMost, -kTiltMost, -kShiftMost, -kShiftMost, -kShiftMost};
        hi = {hand::kElevHard[1] - th.elev0, hand::kAwayHard[1] - th.away0, 2 * hand::kRollFree,
              hand::kMcpHard[1] - th.mcp0.flex, hand::kAbdHard[1], hand::kIpHard[1] - th.ip0.flex, kTiltMost, kTiltMost, kTiltMost, kShiftMost, kShiftMost, kShiftMost};
        out << "[" << side << "]={\n";
        for (size_t kind = 0; kind < std::size(openings); ++kind) {
            auto o = openings[kind];
            if (const char *only = std::getenv("PIN_ONLY"); only && std::string(only) != o.name)
                continue;
            Model fitted = m;
            const auto &body = bodies[kind];
            const double hand = side == 0 ? 1 : -1;
            auto idleScore = [&](V shift) {
                auto candidate = m;
                candidate.prop.p = m.prop.p + shift;
                auto pose = plain(candidate, .94);
                double pen =
                    std::max(0., .0015 - body.clearance(pose.prop, pose.meta, pose.prox, pose.dist, hand)) * 1000;
                return 200 * pen * pen + dot(shift, shift) * 1e6;
            };
            V idleShift{};
            for (double step : {.003, .001, .0003, .0001})
                for (int pass = 0; pass < 30; ++pass) {
                    double best = idleScore(idleShift);
                    bool changed = false;
                    for (V d : std::array<V, 6>{{{step, 0, 0},
                                                 {-step, 0, 0},
                                                 {0, step, 0},
                                                 {0, -step, 0},
                                                 {0, 0, step},
                                                 {0, 0, -step}}}) {
                        V trial = idleShift + d;
                        double cost = idleScore(trial);
                        if (cost < best) {
                            idleShift = trial;
                            best = cost;
                            changed = true;
                        }
                    }
                    if (!changed)
                        break;
                }
            fitted.prop.p = fitted.prop.p + idleShift;
            Model shut = m;
            shut.meta.q = quat(closed["thumb"][0]);
            shut.prox.q = quat(closed["thumb"][1]);
            shut.dist.q = quat(closed["thumb"][2]);
            shut.prop = {vec(closed["p"]), quat(closed["q"])};
            if (side == 0) {
                shut.prop.p = shut.prop.p * -1;
                Q q = shut.prop.q;
                shut.prop.q = {q.w, q.z, -q.y, -q.x};
            }
            shut.prop.p = shut.prop.p + idleShift;
            Params closedAngles{};
            auto closedCost = [&](const Params &params) {
                Model end = shut;
                end.meta.q = shut.meta.q * euler(params[0], params[1], params[2]);
                end.prox.q = shut.prox.q * euler(0, params[3], params[4]);
                end.dist.q = shut.dist.q * axis(2, params[5]);
                double cost = 0;
                for (double u : {0., .25, .5, .75, 1.}) {
                    Model blendModel = fitted;
                    blendModel.meta.q = blend(fitted.meta.q, end.meta.q, u);
                    blendModel.prox.q = blend(fitted.prox.q, end.prox.q, u);
                    blendModel.dist.q = blend(fitted.dist.q, end.dist.q, u);
                    blendModel.prop.p = fitted.prop.p + (end.prop.p - fitted.prop.p) * u;
                    blendModel.prop.q = blend(fitted.prop.q, end.prop.q, u);
                    auto s = plain(blendModel, .94);
                    double p = std::max(0., .002 - body.clearance(s.prop, s.meta, s.prox, s.dist, hand)) * 1000;
                    cost += 400 * p * p;
                }
                for (int i = 0; i < 6; i++)
                    cost += .03 * params[i] * params[i];
                return cost;
            };
            for (double step : {8., 4., 2., .8, .3, .1, .03})
                for (int pass = 0; pass < 90; ++pass) {
                    double best = closedCost(closedAngles);
                    bool changed = false;
                    for (int i = 0; i < 6; i++)
                        for (double sign : {-1., 1.}) {
                            auto candidate = closedAngles;
                            candidate[i] = std::clamp(candidate[i] + sign * step, -40., 40.);
                            double c = closedCost(candidate);
                            if (c < best) {
                                best = c;
                                closedAngles = candidate;
                                changed = true;
                            }
                        }
                    if (!changed)
                        break;
                }
            shut.meta.q = shut.meta.q * euler(closedAngles[0], closedAngles[1], closedAngles[2]);
            shut.prox.q = shut.prox.q * euler(0, closedAngles[3], closedAngles[4]);
            shut.dist.q = shut.dist.q * axis(2, closedAngles[5]);
            const auto original = o;
            const V hinge = PinBody::pinAxisCentre(meshDir, o.name);
            double turnDegrees = 45, bestGap = -1e9;
            for (double angle = 45; angle <= 170; angle += 5) {
                auto target = hinge + rot(axis(1, side == 0 ? angle : -angle), o.centre - hinge);
                double gap = body.distanceExact(target);
                if (gap > bestGap) {
                    bestGap = gap;
                    turnDegrees = angle;
                }
                if (gap > .012)
                    break;
            }
            const Q turn = axis(1, side == 0 ? turnDegrees : -turnDegrees);
            o.centre = hinge + rot(turn, o.centre - hinge);
            o.normal = rot(turn, o.normal);
            Fit f{fitted, o};
            f.th = &th;
            f.rig = &rig;
            f.side = side;
            f.lo = lo;
            f.hi = hi;
            f.body = &body;
            f.palm = &palm;
            f.handedness = hand;
            {
                double least = 1e9;
                Q qi = inv(fitted.prop.q);
                for (V v : palm)
                    least = std::min(least, body.distance(rot(qi, v - fitted.prop.p)));
                f.palmLeast = least;
                f.fingerSkin = &fingerSkin;
                for (int c = 1; c < 5; ++c) {
                    double l = 1e9;
                    for (int k : moving[c])
                        l = std::min(l, body.distance(rot(qi, holdSkin[k] - fitted.prop.p)));
                    f.fingerFloor[c] = std::min(kMargin, l);
                }
            }
            auto neutral = plain(fitted, .94);
            std::cout << side << " " << o.name << " neutral clearance "
                      << body.clearance(neutral.prop, neutral.meta, neutral.prox, neutral.dist, hand) * 1000
                      << " mm, palm " << f.palmLeast * 1000 << " mm, hinge " << turnDegrees << " degrees\n"
                      << std::flush;
            // Reaching over the ring: the thumb raised off the palm and turned in toward it, the tip bent.
            f.preferred = {10, 0, 0, -10, 0, 0, 0, 0, 0, 0, 0, 0};
            Params contact = f.solve(f.preferred);
            for (auto seed : std::array<Params, 5>{Params{25, 10, 0, -20, 0, -10, 0, -20, 15, 0, 5, 0},
                                                   Params{20, -10, 0, -30, 0, 10, 0, 20, -20, 0, 5, 0},
                                                   Params{35, 5, 10, -40, 5, -20, 10, 0, 10, 0, 0, 0},
                                                   Params{5, 15, -10, -15, -5, 10, -10, 10, 0, 0, 0, 0},
                                                   Params{0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}}) {
                auto candidate = f.solve(seed);
                if (f.cost(candidate) < f.cost(contact))
                    contact = candidate;
            }
            auto c = evaluate(fitted, th, contact, .94);
            const double error = std::sqrt(
                dot(c.at - c.prop.p - rot(c.prop.q, o.centre), c.at - c.prop.p - rot(c.prop.q, o.centre)));
            auto clearance = body.clearance(c.prop, c.meta, c.prox, c.dist, hand);
            std::cout << side << " " << o.name << " contact " << error * 1000 << " mm clearance "
                      << clearance * 1000 << " mm, strain " << f.anatomy(contact) << ", grip tilt "
                      << 2 * std::acos(std::min(1., std::abs(euler(contact[6], contact[7], contact[8]).w))) / kDeg
                      << " deg shift "
                      << std::sqrt(contact[9] * contact[9] + contact[10] * contact[10] + contact[11] * contact[11])
                      << " mm\n"
                      << std::flush;
            {
                Q q[3];
                th.locals(contact.data(), q);
                auto a = th.read(rig, side, q);
                std::cout << "  thumb elev " << a.elev << " away " << a.away << " roll " << a.roll << " mcp " << a.mcp
                          << " abd " << a.abd << " ip " << a.ip << " whole " << a.whole << "; params";
                for (double v : contact)
                    std::cout << " " << v;
                std::cout << '\n';
            }
            {
                V shift = gripOf(fitted, contact).p - fitted.prop.p;
                std::cout << "  grip shift along the fingers " << dot(shift, th.pm.d) * 1000 << " mm, toward the thumb "
                          << dot(shift, th.pm.t) * 1000 << " mm, out of the palm " << dot(shift, th.pm.n) * 1000
                          << " mm\n";
            }
            if (error > .0015 || clearance < -.0002)
                throw std::runtime_error("contact residual");
            struct Stage {
                double time, along, travel, ip;
                bool loose; // over the ring before the hook, following through after the yank: near, not exactly
                double ipMost;
            };
            // ip: the IP's preferred flexion delta -- open over the ring, hooked through it, opening as it pulls.
            // Over the ring the tip is barely bent (it has nothing to hook yet): the IP no more than 50 degrees.
            const Stage stages[] = {{.18, .94, .018, -25, true, 50},  {.34, 1.06, 0, 8, false, 1e9},
                                    {.46, .94, 0, 10, false, 1e9},    {.58, .94, .0025, 8, false, 1e9},
                                    {.70, .94, .016, -15, false, 1e9}, {.77, .94, .026, -25, true, 1e9}};
            std::vector<std::pair<double, Params>> keys{{0, Params{}}};
            Params previous{};
            for (auto stage : stages) {
                Fit solve = f;
                solve.moveProp = false;
                solve.preferred = contact;
                solve.along = stage.along;
                solve.travel = stage.travel;
                // The first key grows out of the hold by the regrip's weight, not along the keys at full weight:
                // its way is checked below on the weighted timeline itself.
                solve.pathFrom = stage.time == .18 ? nullptr : &previous;
                // ...but on the way the runtime plays it: out of the hold by the regrip's weight and back into it,
                // from both ends of the lever.
                static const Params zero{};
                if (stage.time == .18) {
                    solve.holds[0] = &fitted;
                    solve.holds[1] = &shut;
                    solve.segments = {{&zero, nullptr, 0, .18}, {nullptr, &zero, 1.07, 1.27}};
                }
                Params initial = stage.time == .18 ? contact : previous;
                solve.preferred[5] = contact[5] + stage.ip;
                solve.ipMost = stage.ipMost;
                if (stage.loose) {
                    // After the yank the thumb follows through: out along the pull, the metacarpal where the
                    // hook had it and the two joints opening, rather than the whole thumb aiming the tip.
                    solve.targetWeight = tunable("PIN_LOOSE_TARGET", 12);
                    solve.anatomyWeight = tunable("PIN_LOOSE_ANATOMY", 1.2);
                    solve.facingWeight = tunable("PIN_LOOSE_FACING", 300);
                    solve.baseHold = tunable("PIN_LOOSE_BASE", 12);
                }
                auto p = solve.solve(initial);
                auto s = evaluate(fitted, th, p, stage.along);
                V target = s.prop.p + rot(s.prop.q, o.centre) + unit(rot(s.prop.q, o.normal)) * stage.travel;
                Q sq[3];
                th.locals(p.data(), sq);
                auto sa = th.read(rig, side, sq);
                std::cout << "  " << stage.time << " residual " << std::sqrt(dot(s.at - target, s.at - target)) * 1000
                          << " mm, strain " << solve.anatomy(p) << " (elev " << sa.elev << " away " << sa.away
                          << " roll " << sa.roll << " mcp " << sa.mcp << " abd " << sa.abd << " ip " << sa.ip
                          << " whole " << sa.whole << ")\n";
                keys.push_back({stage.time, p});
                previous = p;
            }
            keys.push_back({.93, previous});
            keys.push_back({1.07, keys[1].second});
            keys.push_back({1.27, Params{}});
            // The profile the runtime will read, replayed here exactly as pin_contact.lua plays it.
            pin::Profile prof;
            prof.centre = o.centre;
            prof.normal = unit(o.normal);
            prof.radius = o.radius;
            prof.hinge = hinge;
            prof.pinRotation = turn;
            prof.originalCentre = original.centre;
            prof.idleShift = idleShift;
            prof.tip = m.dist.p * .75;
            for (int i = 0; i < 3; ++i)
                prof.rest[i] = th.rest[i];
            prof.closedRest[0] = shut.meta.q;
            prof.closedRest[1] = shut.prox.q;
            prof.closedRest[2] = shut.dist.q;
            prof.hold = {fitted.prop.p, m.prop.q};
            for (auto &[t, p] : keys) {
                pin::Key k;
                k.t = t;
                deltas(th, p, k.q);
                k.grip = euler(p[6], p[7], p[8]);
                k.shift = {p[9] * .001, p[10] * .001, p[11] * .001};
                prof.keys.push_back(k);
            }
            pin::Base openBase{holdPose, {th.rest[0], th.rest[1], th.rest[2]}, fitted.prop, 0};
            pin::Base shutBase{pinchPose, {shut.meta.q, shut.prox.q, shut.dist.q}, shut.prop, 1};
            // THE FINGERS through the regrip, from either end of the lever.
            {
                const pin::Profile bare = prof;
                for (double lever : {0., .25, .5, .75, 1.}) {
                    const auto between = pin::between(openBase, shutBase, lever - .125);
                    prof.fingers.push_back(fingerTrack(rig, mesh, body, bare, pin::between(openBase, shutBase, lever),
                                                       moving, prof.fingers.empty() ? nullptr : &prof.fingers.back(),
                                                       prof.fingers.empty() ? nullptr : &between));
                }
            }
            {
                const char *names[] = {"index", "middle", "ring", "little"};
                std::array<double, 8> most{};
                for (const auto &track : prof.fingers)
                    for (const auto &f : track)
                        for (int i = 0; i < 8; ++i)
                            if (std::abs(f.a[i]) > std::abs(most[i]))
                                most[i] = f.a[i];
                std::cout << "  fingers, most opened (mcp/pip deg):";
                for (int c = 0; c < 4; ++c)
                    std::cout << " " << names[c] << " " << most[c * 2] << "/" << most[c * 2 + 1];
                std::cout << '\n';
            }
            auto run = pin::simulate(rig, side, prof, openBase);
            auto runShut = pin::simulate(rig, side, prof, shutBase);
            std::cout << "  ring: hooked " << run.hookedAt << " s, released " << run.releasedAt << " s at "
                      << run.force << " N (shut hold " << runShut.releasedAt << " s)"
                      << (run.lost.empty() ? "" : ", lost at " + run.lost) << "\n";
            if (run.releasedAt < 0 || runShut.releasedAt < 0)
                throw std::runtime_error("the thumb never pulls the ring out");
            // THE SNAP: Thumb2 straightens to a slight bend (a thumb snapped straight still curves a little: no
            // hyperextension, the line of the thumb not a rod), as straight as the shell allows; Thumb1 keeps
            // its bend from the pull.
            auto thumbClear = [&](const pin::Profile &p, const pin::Base &base, double from, double to) {
                double worst = 1e9;
                for (double t = from; t <= to + 1e-9; t += .005) {
                    auto s = pin::at(p, base, t, from);
                    auto fk = hand::fk(rig, side, s.pose);
                    worst = std::min(worst, body.clearance(s.prop, fk.bone[0], fk.bone[5], fk.bone[10], hand, true));
                }
                return worst;
            };
            {
                const double release = run.releasedAt;
                auto s = pin::at(prof, openBase, release, -1);
                double chosen = 1e9;
                for (double ip : {8., 12., 18., 26.}) {
                    pin::Profile trial = prof;
                    trial.snaps = true;
                    trial.snapQ[0] = s.pose[5];
                    trial.snapQ[1] = th.rest[2] * axis(2, th.ip0.flex - ip);
                    double worst = std::min(thumbClear(trial, openBase, release, release + .32),
                                            thumbClear(trial, shutBase, runShut.releasedAt, runShut.releasedAt + .32));
                    if (worst > -.0002) {
                        prof = trial;
                        chosen = ip;
                        break;
                    }
                }
                if (chosen > 1e8)
                    throw std::runtime_error("no room to straighten the thumb's tip");
                std::cout << "  snap: IP to " << chosen << " deg (keyed " << hand::bendOf(s.pose[10], hand).flex
                          << "), MCP keeps its bend\n";
            }
            // The whole timeline from both holds: the thumb's capsules, every finger's real skin.
            // movingLeast: [1] the skin no finger moves, [2..5] each finger's own.
            auto movingLeast = [&](const T &prop) {
                std::array<double, 6> l;
                l.fill(1e9);
                Q qi = inv(prop.q);
                for (int ch = 1; ch < 5; ++ch)
                    for (int k : moving[ch])
                        l[ch + 1] = std::min(l[ch + 1], body.distance(rot(qi, mesh.sv[k] - prop.p)));
                for (int k : still)
                    l[1] = std::min(l[1], body.distance(rot(qi, mesh.sv[k] - prop.p)));
                return l;
            };
            {
                const char *names[] = {"", "index", "middle", "ring", "little"};
                double worstThumb = 1e9, worstThumbAt = 0;
                std::array<double, 6> worst, at{}, holdLeast{};
                std::array<int, 6> from{};
                std::array<double, 6> regrip;
                regrip.fill(1e9);
                worst.fill(1e9);
                // The two ends of the lever, and three places between them.
                const double levers[] = {0, 1, .5, .375, .875};
                for (int b = 0; b < 5; ++b) {
                    const auto base = pin::between(openBase, shutBase, levers[b]);
                    double rel = (b ? runShut : run).releasedAt;
                    auto s0 = pin::at(prof, base, 0, rel);
                    mesh.skin(rig, s0.pose);
                    auto c0 = movingLeast(s0.prop);
                    for (double t = 0; t <= pin::kDuration + 1e-9; t += .005) {
                        auto s = pin::at(prof, base, t, rel);
                        auto fk = hand::fk(rig, side, s.pose);
                        double c = body.clearance(s.prop, fk.bone[0], fk.bone[5], fk.bone[10], hand, true);
                        if (c < worstThumb)
                            worstThumb = c, worstThumbAt = t;
                        if (std::fmod(t + 1e-9, .02) > .006)
                            continue;
                        mesh.skin(rig, s.pose);
                        auto skin = movingLeast(s.prop);
                        double w = pin::weight(t);
                        for (int g = 1; g < 6; ++g) {
                            // Never deeper than the hold itself (the regrip's own clearance reported).
                            // g 1 is the palm and the fingers' bases (no finger moves them).
                            double allowed = std::min(c0[g], 0.) - .0003;
                            if (g > 1 && w >= .9)
                                regrip[g] = std::min(regrip[g], skin[g]);
                            double excess = skin[g] - allowed;
                            if (excess < worst[g])
                                worst[g] = excess, at[g] = t, holdLeast[g] = skin[g], from[g] = b;
                        }
                    }
                }
                std::cout << "  trajectory: thumb " << worstThumb * 1000 << " mm at " << worstThumbAt << " s;";
                for (int g = 1; g < 6; ++g)
                    std::cout << " " << (g == 1 ? "palm" : names[g - 1]) << " " << holdLeast[g] * 1000 << " mm at "
                              << at[g] << " lever " << levers[from[g]] << " (" << (worst[g] + .0003) * 1000 << " past the hold)";
                std::cout << "\n  regrip in, least clearance:";
                for (int g = 2; g < 6; ++g)
                    std::cout << " " << names[g - 1] << " " << regrip[g] * 1000 << " mm";
                std::cout << "\n" << std::flush;
                auto fail = [&](const std::string &why) {
                    if (!lenient)
                        throw std::runtime_error(why);
                    std::cout << "  VIOLATION: " << why << '\n';
                    ++violations;
                };
                if (worstThumb < -.0005)
                    fail("Thumb crosses mesh between keys");
                for (int g = 1; g < 6; ++g)
                    // Past the hold's own depth by more than a millimetre and a little, for a frame or two
                    // while the regrip blends in or out: no further.
                    if (worst[g] + .0003 < -.0012)
                        fail(g == 1 ? std::string("the palm goes into the shell")
                                                        : std::string("the ") + names[g - 1] + " finger goes into the shell");
            }
            out << o.name << "={centre=";
            printRow(out, {o.centre.x, o.centre.y, o.centre.z});
            out << ",normal=";
            printRow(out, {o.normal.x, o.normal.y, o.normal.z});
            out << ",radius=" << o.radius << ",tip=";
            printRow(out, {m.dist.p.x * .75, m.dist.p.y * .75, m.dist.p.z * .75});
            out << ",hinge=";
            printRow(out, {hinge.x, hinge.y, hinge.z});
            out << ",pinRotation=";
            printQ(out, turn);
            out << ",originalCentre=";
            printRow(out, {original.centre.x, original.centre.y, original.centre.z});
            out << ",idleShift=";
            printRow(out, {idleShift.x, idleShift.y, idleShift.z});
            out << ",offsets={";
            for (V p : {m.meta.p, m.prox.p, m.dist.p}) {
                printRow(out, {p.x, p.y, p.z});
                out << ",";
            }
            out << "}";
            out << ",rest={";
            for (Q q : {m.meta.q, m.prox.q, m.dist.q}) {
                printQ(out, q);
                out << ",";
            }
            out << "},closedRest={";
            for (Q q : {shut.meta.q, shut.prox.q, shut.dist.q}) {
                printQ(out, q);
                out << ",";
            }
            out << "},hold={";
            printRow(out, {fitted.prop.p.x, fitted.prop.p.y, fitted.prop.p.z});
            out << ",";
            printQ(out, m.prop.q);
            out << "},snap={";
            printQ(out, prof.snapQ[0]);
            out << ",";
            printQ(out, prof.snapQ[1]);
            out << "},release=" << run.releasedAt << ",keys={\n";
            for (const auto &k : prof.keys) {
                out << "{t=" << k.t << ",q={";
                for (int i = 0; i < 3; ++i) {
                    printQ(out, k.q[i]);
                    out << (i < 2 ? "," : "");
                }
                out << "},grip={";
                printQ(out, k.grip);
                out << ",";
                printRow(out, {k.shift.x, k.shift.y, k.shift.z});
                out << "}},\n";
            }
            out << "},fingers={";
            for (const auto &track : prof.fingers) {
                out << "{\n";
                for (const auto &f : track) {
                    out << std::setprecision(2);
                    printRow(out, {f.t, f.a[0], f.a[1], f.a[2], f.a[3], f.a[4], f.a[5], f.a[6], f.a[7]});
                    out << std::setprecision(8) << ",";
                }
                out << "},";
            }
            out << "}},\n";
        }
        out << "},\n";
    }
    out << "}\n";
    std::ofstream file(argv[2]);
    file << out.str();
    file.close();
    if (violations)
        std::cout << violations << " violations\n";
    return file && !violations ? 0 : 1;
} catch (const std::exception &e) {
    std::cerr << e.what() << '\n';
    return 1;
}
