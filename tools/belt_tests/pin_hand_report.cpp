// The holding hand through the pin timeline of a baked profile (grenade/poses/pin_contact.lua as JSON,
// tools/belt_tests/lua_json.lua): the thumb's anatomical angles at each key, the ring's hook and release
// replayed with the game's spring, the real first-person skin's clearance to the grenade per finger along the
// whole timeline from both holds, and pictures of it.
//
//   pin_hand_report REPO profile.json OUT [--flick ring_flick.json] [--render kind[,kind]] [--summary file.json]
//                   [--video frames]
//
// --video renders the rendered kinds as frame sequences instead (OUT/video_<side>_<kind>/f0000.ppm...), that many
// frames evenly over the timeline, the three views side by side; the ring flies off along its normal.
//
// REPO is the repository root (rig, hand meshes, poses, fixtures, build/pin-clearance/json).
#include "pin_runtime.hpp"
#include <algorithm>
#include <cstdio>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <nlohmann/json.hpp>
#include <set>
#include <sstream>
#include <string>
#include <vector>

namespace fs = std::filesystem;
using hand::kDeg;

const char *const kKinds[] = {"frag", "ozob", "flash", "incendiary", "biohazard", "smoke"};
const char *const kGroups[] = {"palm", "thumb", "index", "middle", "ring", "little"};

struct Paths {
    fs::path repo, rig, meshL, meshR, hold, pinch, fixtures, meshes;
    explicit Paths(fs::path r) : repo(r) {
        rig = r / "tools/grasp_tests/rig.txt";
        meshL = r / "tools/grasp_tests/mesh/hand_l.bin";
        meshR = r / "tools/grasp_tests/mesh/hand_r.bin";
        hold = r / "mods/cet/CyberpunkVRPort_TacticalBelt/grenade/poses/grenade_hold.lua";
        pinch = r / "mods/cet/CyberpunkVRPort_TacticalBelt/grenade/poses/grenade_pinch.lua";
        fixtures = r / "tools/belt_tests/fixtures/pin-contact";
        meshes = r / "build/pin-clearance/json";
    }
};

Json readJson(const fs::path &p) {
    std::ifstream f(p);
    if (!f)
        throw std::runtime_error("missing " + p.string());
    Json j;
    f >> j;
    return j;
}

// The closed hold's grenade (closed.json is the right hand's; the left is mirrored as grenadeHoldPose does).
T closedProp(const Json &closed, int side) {
    T t{pin::vecOf(closed["p"]), pin::quatOf(closed["q"])};
    if (side == 0) {
        t.p = t.p * -1;
        Q q = t.q;
        t.q = {q.w, q.z, -q.y, -q.x};
    }
    return t;
}

// ---------------------------------------------------------------- pictures

struct Canvas {
    int w, h;
    std::vector<float> rgb, z;
    Canvas(int W, int H) : w(W), h(H), rgb(size_t(W) * H * 3, 0), z(size_t(W) * H, 1e9f) {}
};
struct Camera {
    V c, r, u, f;
    double extent;
};
struct Tri {
    V p[3];
    V col[3];
};
void raster(Canvas &cv, const Camera &cam, int x0, int size, const std::vector<Tri> &tris) {
    for (const auto &t : tris) {
        double X[3], Y[3], Z[3];
        for (int i = 0; i < 3; ++i) {
            V d = t.p[i] - cam.c;
            X[i] = (dot(d, cam.r) / cam.extent + .5) * size - .5;
            Y[i] = (.5 - dot(d, cam.u) / cam.extent) * size - .5;
            Z[i] = dot(d, cam.f);
        }
        double area = (X[1] - X[0]) * (Y[2] - Y[0]) - (Y[1] - Y[0]) * (X[2] - X[0]);
        if (std::abs(area) < 1e-9)
            continue;
        V nrm = unit(cross(t.p[1] - t.p[0], t.p[2] - t.p[0]));
        double light = .3 + .7 * std::abs(dot(nrm, cam.f));
        int xmin = std::max(0, int(std::floor(std::min({X[0], X[1], X[2]})))),
            xmax = std::min(size - 1, int(std::ceil(std::max({X[0], X[1], X[2]}))));
        int ymin = std::max(0, int(std::floor(std::min({Y[0], Y[1], Y[2]})))),
            ymax = std::min(cv.h - 1, int(std::ceil(std::max({Y[0], Y[1], Y[2]}))));
        for (int y = ymin; y <= ymax; ++y)
            for (int x = xmin; x <= xmax; ++x) {
                double w0 = ((X[1] - x) * (Y[2] - y) - (Y[1] - y) * (X[2] - x)) / area;
                double w1 = ((X[2] - x) * (Y[0] - y) - (Y[2] - y) * (X[0] - x)) / area;
                double w2 = 1 - w0 - w1;
                if (w0 < 0 || w1 < 0 || w2 < 0)
                    continue;
                double zz = w0 * Z[0] + w1 * Z[1] + w2 * Z[2];
                size_t at = size_t(y) * cv.w + x0 + x;
                if (zz >= cv.z[at])
                    continue;
                cv.z[at] = float(zz);
                V col = t.col[0] * w0 + t.col[1] * w1 + t.col[2] * w2;
                cv.rgb[at * 3] = float(col.x * light);
                cv.rgb[at * 3 + 1] = float(col.y * light);
                cv.rgb[at * 3 + 2] = float(col.z * light);
            }
    }
}
// The hand over the grenade, the grenade see-through where it covers the hand.
void composite(Canvas &out, const Canvas &handC, const Canvas &bodyC) {
    for (size_t i = 0; i < out.z.size(); ++i) {
        bool hh = handC.z[i] < 1e8f, bb = bodyC.z[i] < 1e8f;
        for (int c = 0; c < 3; ++c) {
            float v = .12f;
            if (hh && (!bb || handC.z[i] <= bodyC.z[i]))
                v = handC.rgb[i * 3 + c];
            else if (bb && hh)
                v = .5f * bodyC.rgb[i * 3 + c] + .5f * handC.rgb[i * 3 + c];
            else if (bb)
                v = bodyC.rgb[i * 3 + c];
            out.rgb[i * 3 + c] = v;
        }
    }
}
void writePpm(const fs::path &p, const Canvas &cv) {
    std::ofstream f(p, std::ios::binary);
    f << "P6\n" << cv.w << " " << cv.h << "\n255\n";
    for (size_t i = 0; i < cv.rgb.size(); ++i)
        f.put(char(std::clamp(int(cv.rgb[i] * 255), 0, 255)));
}

std::vector<Tri> meshTris(const PinMesh &m, const T &at, V col) {
    std::vector<Tri> out;
    out.reserve(m.triangles.size());
    for (const auto &t : m.triangles) {
        Tri r;
        V v[3] = {t.a, t.b, t.c};
        for (int i = 0; i < 3; ++i) {
            r.p[i] = at.p + rot(at.q, v[i]);
            r.col[i] = col;
        }
        out.push_back(r);
    }
    return out;
}

int main(int argc, char **argv) try {
    if (argc < 4)
        throw std::runtime_error("REPO profile.json OUT [--flick ring_flick.json] [--render kinds] [--summary f]");
    Paths P(argv[1]);
    Json profiles = readJson(argv[2]);
    fs::path out = argv[3];
    fs::create_directories(out);
    std::optional<pin::Flick> flick;
    std::set<std::string> render;
    fs::path summaryPath;
    int videoFrames = 0;
    for (int i = 4; i + 1 < argc; i += 2) {
        std::string a = argv[i];
        if (a == "--flick")
            flick = pin::Flick::load(readJson(argv[i + 1]));
        else if (a == "--render") {
            std::stringstream s(argv[i + 1]);
            std::string k;
            while (std::getline(s, k, ','))
                render.insert(k);
        } else if (a == "--summary")
            summaryPath = argv[i + 1];
        else if (a == "--video")
            videoFrames = std::stoi(argv[i + 1]);
    }
    auto rig = hand::Rig::load(P.rig);
    auto holdPose = hand::loadPose(P.hold), pinchPose = hand::loadPose(P.pinch);
    Json closed = readJson(P.fixtures / "closed.json");
    hand::HandMesh meshes[2] = {hand::HandMesh::load(P.meshL, 0, rig), hand::HandMesh::load(P.meshR, 1, rig)};
    std::cout << std::fixed << std::setprecision(2);
    // The rig's FK against the hand the game held (fixtures): thumb and index chains.
    for (int side = 0; side < 2; ++side) {
        Json fx = readJson(P.fixtures / (side ? "right.json" : "left.json"));
        std::vector<T> b;
        for (auto &v : fx["bones"])
            b.push_back({pin::vecOf(v["p"]), pin::quatOf(v["q"])});
        hand::Pose pose = holdPose;
        for (int k = 0; k < 3; ++k)
            pose[hand::kChain[0][k]] = relative(k ? b[k] : b[0], b[k + 1]).q;
        auto f = hand::fk(rig, side, pose);
        const int slots[7] = {0, 5, 10, 1, 6, 11, 15};
        double worstP = 0, worstQ = 0;
        for (int k = 0; k < 7; ++k) {
            T want = relative(b[0], b[k + 1]);
            V d = want.p - f.bone[slots[k]].p;
            worstP = std::max(worstP, std::sqrt(dot(d, d)));
            worstQ = std::max(worstQ, hand::angleBetween(want.q, f.bone[slots[k]].q) / kDeg);
        }
        std::cout << "fk " << side << " against the game's hand: " << worstP * 1000 << " mm, " << worstQ
                  << " deg; mesh bind " << meshes[side].bindError(rig) * 1000 << " mm; vertices";
        for (int g = 0; g < 6; ++g)
            std::cout << " " << kGroups[g] << " " << meshes[side].byGroup[g].size();
        std::cout << "\n";
    }
    Json summary;
    for (int side = 0; side < 2; ++side)
        for (const char *kind : kKinds) {
            if (!profiles.contains(std::to_string(side)) || !profiles[std::to_string(side)].contains(kind))
                continue;
            auto p = pin::parse(profiles[std::to_string(side)][kind]);
            auto body = PinBody::load(P.meshes, kind);
            body.buildField();
            auto &mesh = meshes[side];
            pin::Base open{holdPose, {p.rest[0], p.rest[1], p.rest[2]}, p.hold, 0};
            T shut = closedProp(closed, side);
            shut.p = shut.p + p.idleShift;
            pin::Base closedBase{pinchPose, {p.closedRest[0], p.closedRest[1], p.closedRest[2]}, shut, 1};
            hand::Pose restPose = holdPose;
            for (int k = 0; k < 3; ++k)
                restPose[hand::kChain[0][k]] = p.rest[k];
            auto thumb = hand::Thumb::make(rig, side, restPose);
            std::cout << "\n== " << (side ? "right" : "left") << " " << kind << "  rest elev " << thumb.elev0
                      << " away " << thumb.away0 << " mcp " << thumb.mcp0.flex << " ip " << thumb.ip0.flex << "\n";
            Json js;
            double worstStrain = 0;
            bool hard = true;
            for (const auto &k : p.keys) {
                Q q[3];
                for (int i = 0; i < 3; ++i)
                    q[i] = p.rest[i] * k.q[i];
                auto a = thumb.read(rig, side, q);
                double strain = thumb.strain(a);
                worstStrain = std::max(worstStrain, strain);
                hard = hard && thumb.withinHard(a);
                double tilt = 2 * std::acos(std::min(1., std::abs(k.grip.w))) / kDeg;
                std::cout << "  key " << std::setw(4) << k.t << "  elev " << std::setw(6) << a.elev << " away "
                          << std::setw(6) << a.away << " roll " << std::setw(6) << a.roll << " | mcp "
                          << std::setw(6) << a.mcp << " abd " << std::setw(6) << a.abd << " twist " << std::setw(6)
                          << a.twist << " | ip " << std::setw(6) << a.ip << " whole " << std::setw(6) << a.whole
                          << " | strain " << std::setw(7) << strain << (thumb.withinHard(a) ? "" : " OUT")
                          << " | grip tilt " << tilt << " shift " << std::sqrt(dot(k.shift, k.shift)) * 1000
                          << " mm\n";
            }
            js["strain"] = worstStrain;
            js["withinHard"] = hard;
            pin::Run runs[2];
            for (int b = 0; b < 2; ++b) {
                runs[b] = pin::simulate(rig, side, p, b ? closedBase : open, 1. / 90, flick ? &*flick : nullptr);
                const auto &r = runs[b];
                std::cout << "  ring from the " << (b ? "shut" : "open") << " hold: hooked " << r.hookedAt
                          << " s, released " << r.releasedAt << " s at " << r.force << " N, pull "
                          << r.pullAtRelease * 1000 << " mm, launch " << r.launch << " m/s"
                          << (r.lost.empty() ? "" : ", lost at " + r.lost) << "\n";
                js[b ? "shut" : "open"]["released"] = r.releasedAt;
                js[b ? "shut" : "open"]["hooked"] = r.hookedAt;
            }
            for (int b = 0; b < 2; ++b) {
                const auto &base = b ? closedBase : open;
                std::array<double, 6> worst;
                std::array<double, 6> when{};
                worst.fill(1e9);
                double rel = runs[b].releasedAt;
                for (double t = 0; t <= pin::kDuration + 1e-9; t += .01) {
                    auto s = pin::at(p, base, t, rel, flick ? &*flick : nullptr);
                    mesh.skin(rig, s.pose);
                    auto c = hand::clearance(mesh, body, s.prop);
                    for (int g = 0; g < 6; ++g)
                        if (c.least[g] < worst[g]) {
                            worst[g] = c.least[g];
                            when[g] = t;
                        }
                }
                std::cout << "  skin from the " << (b ? "shut" : "open") << " hold, least clearance (mm @ s):";
                for (int g = 0; g < 6; ++g) {
                    // The worst sample again against the triangles, not the sampled field.
                    auto s = pin::at(p, base, when[g], rel, flick ? &*flick : nullptr);
                    mesh.skin(rig, s.pose);
                    auto c = hand::clearance(mesh, body, s.prop, true, g);
                    worst[g] = c.least[g];
                    std::cout << " " << kGroups[g] << " " << worst[g] * 1000 << "@" << when[g];
                    js[b ? "shut" : "open"]["skin"][kGroups[g]] = worst[g] * 1000;
                    js[b ? "shut" : "open"]["at"][kGroups[g]] = when[g];
                }
                std::cout << "\n";
            }
            // At the keys, from the open hold (what the weighted middle of the timeline shows).
            std::cout << "  skin at the keys (mm, inside vertices):\n";
            for (const auto &k : p.keys) {
                auto s = pin::at(p, open, k.t, -1, flick ? &*flick : nullptr);
                mesh.skin(rig, s.pose);
                auto c = hand::clearance(mesh, body, s.prop, true);
                std::cout << "    " << std::setw(4) << k.t;
                for (int g = 0; g < 6; ++g)
                    std::cout << "  " << kGroups[g] << " " << std::setw(6) << c.least[g] * 1000 << " ("
                              << c.inside[g] << ")";
                std::cout << "\n";
            }
            summary[std::to_string(side)][kind] = js;
            if (!render.count(kind) && !render.count("all"))
                continue;
            // Pictures: rows of times, columns of views (into the palm, the thumb's side, three quarters).
            auto pinMesh = PinMesh::load(P.meshes / (PinBody::prefix(kind) + "_safe_pin_01.mesh.json"), false);
            std::vector<double> times = {0, .18, .46, std::max(.5, runs[0].releasedAt - .03), runs[0].releasedAt + .03,
                                         runs[0].releasedAt + .1};
            if (videoFrames > 0) {
                times.clear();
                for (int i = 0; i < videoFrames; ++i)
                    times.push_back(pin::kDuration * i / (videoFrames - 1));
            }
            const int S = videoFrames > 0 ? 360 : 300, rows = int(times.size()), cols = 3;
            const fs::path frameDir = out / ("video_" + std::string(side ? "right_" : "left_") + kind);
            if (videoFrames > 0)
                fs::create_directories(frameDir);
            Canvas sheet(S * cols, videoFrames > 0 ? S : S * rows);
            V lo{1e9, 1e9, 1e9}, hi{-1e9, -1e9, -1e9};
            for (const auto &m : body.meshes) {
                lo = PinMesh::minimum(lo, m.nodes[0].low);
                hi = PinMesh::maximum(hi, m.nodes[0].high);
            }
            V centre = (lo + hi) * .5;
            for (int row = 0; row < rows; ++row) {
                double t = std::clamp(times[row], 0., pin::kDuration);
                double rel = runs[0].releasedAt;
                auto s = pin::at(p, open, t, rel);
                mesh.skin(rig, s.pose);
                std::vector<Tri> handTris, bodyTris;
                std::vector<double> depth(mesh.sv.size(), 1);
                for (size_t k = 0; k < mesh.sv.size(); ++k)
                    if (mesh.used[k])
                        depth[k] = body.distance(rot(inv(s.prop.q), mesh.sv[k] - s.prop.p));
                for (size_t t3 = 0; t3 < mesh.draw.size(); ++t3) {
                    if (!mesh.draw[t3])
                        continue;
                    Tri tr;
                    for (int c = 0; c < 3; ++c) {
                        int k = int(mesh.tri[t3 * 3 + c]);
                        tr.p[c] = mesh.sv[k];
                        V col = mesh.group[k] == 1 ? V{.92, .55, .45} : V{.86, .70, .60};
                        if (depth[k] < 0)
                            col = {1, .1, .9};
                        tr.col[c] = col;
                    }
                    handTris.push_back(tr);
                }
                for (const auto &m : body.meshes)
                    for (auto &tr : meshTris(m, s.prop, {.45, .55, .5}))
                        bodyTris.push_back(tr);
                // After the release the ring flies on along its normal at the launch speed for a quarter second.
                bool pinIn = rel < 0 || t < rel + (videoFrames > 0 ? .25 : 0);
                if (pinIn) {
                    double u = pin::smooth(t / .22);
                    Q turn = blend(Q{}, p.pinRotation, u);
                    double pull = 0;
                    for (auto &[pt, pv] : runs[0].pull)
                        if (pt <= t)
                            pull = pv;
                    if (rel >= 0 && t >= rel)
                        pull = runs[0].pullAtRelease + runs[0].launch * (t - rel);
                    T local{p.hinge - rot(turn, p.hinge) + p.normal * pull, turn};
                    for (auto &tr : meshTris(pinMesh, compose(s.prop, local), {.95, .8, .25}))
                        bodyTris.push_back(tr);
                }
                auto pm = thumb.pm;
                V c = s.prop.p + rot(s.prop.q, centre) + pm.n * -.01;
                Camera cams[3] = {{c, {}, pm.d, pm.n * -1, .17},
                                  {c, {}, pm.d, pm.t * -1, .17},
                                  {c, {}, pm.d, unit(pm.n * -.6 + pm.t * -.8), .17}};
                for (int col = 0; col < cols; ++col) {
                    Camera cam = cams[col];
                    cam.u = unit(cam.u - cam.f * dot(cam.u, cam.f));
                    cam.r = cross(cam.f, cam.u);
                    Canvas hc(S, S), bc(S, S), oc(S, S);
                    raster(hc, cam, 0, S, handTris);
                    raster(bc, cam, 0, S, bodyTris);
                    composite(oc, hc, bc);
                    const int at = videoFrames > 0 ? 0 : row;
                    for (int y = 0; y < S; ++y)
                        for (int x = 0; x < S; ++x)
                            for (int ch = 0; ch < 3; ++ch)
                                sheet.rgb[((size_t(at) * S + y) * sheet.w + col * S + x) * 3 + ch] =
                                    oc.rgb[(size_t(y) * S + x) * 3 + ch];
                }
                if (videoFrames > 0) {
                    char name[32];
                    std::snprintf(name, sizeof name, "f%04d.ppm", row);
                    writePpm(frameDir / name, sheet);
                }
                js["frames"].push_back(t);
            }
            if (videoFrames > 0)
                continue;
            summary[std::to_string(side)][kind]["frames"] = js["frames"];
            writePpm(out / (std::string(side ? "right_" : "left_") + kind + ".ppm"), sheet);
        }
    if (!summaryPath.empty()) {
        std::ofstream f(summaryPath);
        f << summary.dump(1);
    }
    return 0;
} catch (const std::exception &e) {
    std::cerr << e.what() << "\n";
    return 1;
}
