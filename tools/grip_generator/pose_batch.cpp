// Per-mesh grasp poses for every hand-sized cooked prop mesh.
//
// For each mesh: its drawn surface as a signed distance field (built here from the decoded triangles), the production
// seat (src/Anim/GraspPose.cpp Seat) on the primitive the game takes the mesh by -- its bounding box, or for a round
// mesh (a radius profile in the catalog) the cylinder round the whole of it and each part GraspPartOf finds on it (a
// neck, a stem, a body), as src/Natives/HeldBody.cpp does -- for each way it can lie against the palm, and the production
// finger solver closed on the drawn surface itself (g_graspSurfaceDistance) instead of the primitive. Each distinct seat is then shifted and tilted to the best fit against the visible FPP hand skin and kept when no
// skin is in the mesh, the pads that should touch do, and every joint stays in its functional range. All numerical work
// runs here in C++; orchestration only shards the candidate list and collects files.
//
//   grip_pose_batch RIG OODLE_DLL CANDIDATES.jsonl OUT.jsonl SHARD SHARDS [PREVIEW_DIR PREVIEW_EVERY]
//   grip_pose_batch --pack OUT.bin IN.jsonl...
//
// A candidate line holds hash, path, minimum, maximum (from the geometry catalog) and source (the extracted file).
#include "cr2w_mesh.hpp"
#undef near   // windows.h; the harness has variables of that name
#undef far
#define main GraspVisualHarnessMain
#include "../grasp_tests/harness.cpp"
#undef main
#include <charconv>
#include <filesystem>
#include <iomanip>
#include <iostream>
#include <map>

namespace batch {
using Json = nlohmann::json;
static void Need(bool ok, const char* why) { if (!ok) throw std::runtime_error(why); }

// ---- the drawn surface: signed distance on a regular grid in the mesh's own frame ----
struct Field {
    float lo[3]{}, cell = 0.f;
    int n[3]{};
    std::vector<float> d;
    float Raw(int x, int y, int z) const { return d[(size_t(z) * n[1] + y) * n[0] + x]; }
    float At(const float* p) const {
        float g[3], clamped[3], out2 = 0;
        for (int k = 0; k < 3; ++k) {
            const float hi = lo[k] + cell * (n[k] - 1);
            clamped[k] = std::clamp(p[k], lo[k], hi);
            out2 += (p[k] - clamped[k]) * (p[k] - clamped[k]);
            g[k] = (clamped[k] - lo[k]) / cell;
        }
        int i[3]; float f[3];
        for (int k = 0; k < 3; ++k) { i[k] = std::min(int(g[k]), n[k] - 2); f[k] = g[k] - i[k]; }
        float v = 0;
        for (int c = 0; c < 8; ++c) {
            const int dx = c & 1, dy = c >> 1 & 1, dz = c >> 2 & 1;
            const float w = (dx ? f[0] : 1 - f[0]) * (dy ? f[1] : 1 - f[1]) * (dz ? f[2] : 1 - f[2]);
            v += w * Raw(i[0] + dx, i[1] + dy, i[2] + dz);
        }
        return v + std::sqrt(out2);
    }
};
// Closest point on a triangle (Ericson, Real-Time Collision Detection 5.1.5), the squared distance to it.
static float TriangleDistance2(const float* p, const float* a, const float* b, const float* c) {
    float ab[3], ac[3], ap[3];
    Sub(b, a, ab); Sub(c, a, ac); Sub(p, a, ap);
    const float d1 = Dot(ab, ap), d2 = Dot(ac, ap);
    float q[3];
    auto done = [&](const float* x) { float r[3]; Sub(p, x, r); return Dot(r, r); };
    if (d1 <= 0 && d2 <= 0) return done(a);
    float bp[3]; Sub(p, b, bp);
    const float d3 = Dot(ab, bp), d4 = Dot(ac, bp);
    if (d3 >= 0 && d4 <= d3) return done(b);
    const float vc = d1 * d4 - d3 * d2;
    if (vc <= 0 && d1 >= 0 && d3 <= 0) { const float v = d1 / (d1 - d3); Mad(a, ab, v, q); return done(q); }
    float cp[3]; Sub(p, c, cp);
    const float d5 = Dot(ab, cp), d6 = Dot(ac, cp);
    if (d6 >= 0 && d5 <= d6) return done(c);
    const float vb = d5 * d2 - d1 * d6;
    if (vb <= 0 && d2 >= 0 && d6 <= 0) { const float w = d2 / (d2 - d6); Mad(a, ac, w, q); return done(q); }
    const float va = d3 * d6 - d5 * d4;
    if (va <= 0 && (d4 - d3) >= 0 && (d5 - d6) >= 0) {
        float bc[3]; Sub(c, b, bc); const float w = (d4 - d3) / ((d4 - d3) + (d5 - d6)); Mad(b, bc, w, q); return done(q);
    }
    const float denom = 1.f / (va + vb + vc), v = vb * denom, w = vc * denom;
    for (int k = 0; k < 3; ++k) q[k] = a[k] + ab[k] * v + ac[k] * w;
    return done(q);
}
// Unsigned distance within a band round every triangle; the sign by ray parity along each axis, the majority of three
// (a cooked prop is rarely watertight: a label strip or an open rim flips one direction, seldom two).
static Field BuildField(const gripgen::Mesh& mesh, const float* boxLo, const float* boxHi) {
    Field F;
    float extent = 0;
    for (int k = 0; k < 3; ++k) extent = std::max(extent, boxHi[k] - boxLo[k]);
    F.cell = std::clamp(extent / 90.f, .0015f, .008f);
    const float margin = .035f;
    for (int k = 0; k < 3; ++k) {
        F.lo[k] = boxLo[k] - margin;
        F.n[k] = std::min(220, int(std::ceil((boxHi[k] - boxLo[k] + 2 * margin) / F.cell)) + 1);
    }
    const size_t cells = size_t(F.n[0]) * F.n[1] * F.n[2];
    F.d.assign(cells, 1e9f);
    const float band = std::max(4 * F.cell, .008f);
    std::vector<float> V(mesh.vertices.size() * 3);
    for (size_t i = 0; i < mesh.vertices.size(); ++i) { V[3 * i] = mesh.vertices[i].x; V[3 * i + 1] = mesh.vertices[i].y; V[3 * i + 2] = mesh.vertices[i].z; }
    auto index = [&](float v, int k) { return int(std::floor((v - F.lo[k]) / F.cell)); };
    for (const auto& t : mesh.triangles) {
        const float *a = &V[3 * t[0]], *b = &V[3 * t[1]], *c = &V[3 * t[2]];
        int i0[3], i1[3];
        for (int k = 0; k < 3; ++k) {
            const float mn = std::min({a[k], b[k], c[k]}) - band, mx = std::max({a[k], b[k], c[k]}) + band;
            i0[k] = std::max(0, index(mn, k)); i1[k] = std::min(F.n[k] - 1, index(mx, k) + 1);
        }
        for (int z = i0[2]; z <= i1[2]; ++z)
            for (int y = i0[1]; y <= i1[1]; ++y)
                for (int x = i0[0]; x <= i1[0]; ++x) {
                    const float p[3]{F.lo[0] + x * F.cell, F.lo[1] + y * F.cell, F.lo[2] + z * F.cell};
                    float& cell = F.d[(size_t(z) * F.n[1] + y) * F.n[0] + x];
                    const float d2 = TriangleDistance2(p, a, b, c);
                    if (d2 < cell) cell = d2;
                }
    }
    for (float& v : F.d) v = v >= 1e8f ? 1e9f : std::sqrt(v);
    // Past the band: a chamfer distance (two passes of the 26 neighbours, face/edge/corner steps), so the solver's
    // gradient still points at the surface from a finger or a thumb goal several centimetres off it.
    {
        const float w[3]{F.cell, F.cell * 1.41421356f, F.cell * 1.7320508f};
        const auto at = [&](int x, int y, int z) -> float& { return F.d[(size_t(z) * F.n[1] + y) * F.n[0] + x]; };
        for (int pass = 0; pass < 2; ++pass) {
            const int s = pass ? -1 : 1;
            for (int zi = 0; zi < F.n[2]; ++zi) {
                const int z = pass ? F.n[2] - 1 - zi : zi;
                for (int yi = 0; yi < F.n[1]; ++yi) {
                    const int y = pass ? F.n[1] - 1 - yi : yi;
                    for (int xi = 0; xi < F.n[0]; ++xi) {
                        const int x = pass ? F.n[0] - 1 - xi : xi;
                        float best = at(x, y, z);
                        for (int dz = -1; dz <= 0; ++dz)
                            for (int dy = -1; dy <= 1; ++dy)
                                for (int dx = -1; dx <= 1; ++dx) {
                                    if (dz == 0 && (dy > 0 || (dy == 0 && dx >= 0))) continue;   // the 13 already swept
                                    const int X = x + s * dx, Y = y + s * dy, Z = z + s * dz;
                                    if (X < 0 || Y < 0 || Z < 0 || X >= F.n[0] || Y >= F.n[1] || Z >= F.n[2]) continue;
                                    const float v = at(X, Y, Z) + w[std::abs(dx) + std::abs(dy) + std::abs(dz) - 1];
                                    if (v < best) best = v;
                                }
                        at(x, y, z) = best;
                    }
                }
            }
        }
        for (float& v : F.d) if (v >= 1e8f) v = band;
    }
    // parity: for each axis, every grid line along it, the crossings of the triangles it passes through
    std::vector<uint8_t> votes(cells, 0);
    for (int axis = 0; axis < 3; ++axis) {
        const int b = (axis + 1) % 3, c = (axis + 2) % 3;
        std::vector<std::vector<float>> rows(size_t(F.n[b]) * F.n[c]);
        for (const auto& t : mesh.triangles) {
            const float *P[3]{&V[3 * t[0]], &V[3 * t[1]], &V[3 * t[2]]};
            float mnb = 1e9f, mxb = -1e9f, mnc = 1e9f, mxc = -1e9f;
            for (auto* q : P) { mnb = std::min(mnb, q[b]); mxb = std::max(mxb, q[b]); mnc = std::min(mnc, q[c]); mxc = std::max(mxc, q[c]); }
            const int jb0 = std::max(0, int(std::ceil((mnb - F.lo[b]) / F.cell))), jb1 = std::min(F.n[b] - 1, int(std::floor((mxb - F.lo[b]) / F.cell)));
            const int jc0 = std::max(0, int(std::ceil((mnc - F.lo[c]) / F.cell))), jc1 = std::min(F.n[c] - 1, int(std::floor((mxc - F.lo[c]) / F.cell)));
            const double ub[3]{P[0][b], P[1][b], P[2][b]}, uc[3]{P[0][c], P[1][c], P[2][c]};
            const double area = (ub[1] - ub[0]) * (uc[2] - uc[0]) - (ub[2] - ub[0]) * (uc[1] - uc[0]);
            if (std::fabs(area) < 1e-14) continue;
            for (int jc = jc0; jc <= jc1; ++jc)
                for (int jb = jb0; jb <= jb1; ++jb) {
                    // a half-cell offset keeps the lines off shared edges and vertices
                    const double yb = F.lo[b] + jb * F.cell + 1.3e-6, yc = F.lo[c] + jc * F.cell + 0.7e-6;
                    const double w0 = ((ub[1] - yb) * (uc[2] - yc) - (ub[2] - yb) * (uc[1] - yc)) / area;
                    const double w1 = ((ub[2] - yb) * (uc[0] - yc) - (ub[0] - yb) * (uc[2] - yc)) / area;
                    const double w2 = 1 - w0 - w1;
                    if (w0 < 0 || w1 < 0 || w2 < 0) continue;
                    rows[size_t(jc) * F.n[b] + jb].push_back(float(w0 * P[0][axis] + w1 * P[1][axis] + w2 * P[2][axis]));
                }
        }
        for (int jc = 0; jc < F.n[c]; ++jc)
            for (int jb = 0; jb < F.n[b]; ++jb) {
                auto& xs = rows[size_t(jc) * F.n[b] + jb];
                if (xs.size() < 2) continue;
                std::sort(xs.begin(), xs.end());
                size_t crossed = 0;
                for (int i = 0; i < F.n[axis]; ++i) {
                    const float at = F.lo[axis] + i * F.cell;
                    while (crossed < xs.size() && xs[crossed] < at) ++crossed;
                    if (crossed & 1) {
                        int g[3]; g[axis] = i; g[b] = jb; g[c] = jc;
                        ++votes[(size_t(g[2]) * F.n[1] + g[1]) * F.n[0] + g[0]];
                    }
                }
            }
    }
    for (size_t i = 0; i < cells; ++i) if (votes[i] >= 2) F.d[i] = -F.d[i];
    return F;
}

// The solver's shape frame in the mesh's: a box's is the mesh's axes at the box's centre, a cylinder's its axis on Z.
struct Surface { const Field* field; Pose shape; };
static float SurfaceDistance(const float* local, void* context) {
    const auto* s = static_cast<const Surface*>(context);
    float p[3]; Rot(s->shape.q, local, p);
    for (int k = 0; k < 3; ++k) p[k] += s->shape.p[k];
    return s->field->At(p);
}
// What a hand is put to: the primitive, its half sizes and pose in the mesh, a cylinder's ends and part, the lathe round it.
struct Target { int shape = kBox; float half[3]{}; Pose inMesh; int ends = 15; bool part = false, neck = false, stem = false; GraspLathe lathe; };
static void MatrixQuat(const float (*m)[3], float* q) {
    const float tr = m[0][0] + m[1][1] + m[2][2];
    if (tr > 0) { const float s = std::sqrt(tr + 1) * 2; q[3] = .25f * s; q[0] = (m[2][1] - m[1][2]) / s; q[1] = (m[0][2] - m[2][0]) / s; q[2] = (m[1][0] - m[0][1]) / s; }
    else if (m[0][0] > m[1][1] && m[0][0] > m[2][2]) { const float s = std::sqrt(1 + m[0][0] - m[1][1] - m[2][2]) * 2; q[3] = (m[2][1] - m[1][2]) / s; q[0] = .25f * s; q[1] = (m[0][1] + m[1][0]) / s; q[2] = (m[0][2] + m[2][0]) / s; }
    else if (m[1][1] > m[2][2]) { const float s = std::sqrt(1 + m[1][1] - m[0][0] - m[2][2]) * 2; q[3] = (m[0][2] - m[2][0]) / s; q[0] = (m[0][1] + m[1][0]) / s; q[1] = .25f * s; q[2] = (m[1][2] + m[2][1]) / s; }
    else { const float s = std::sqrt(1 + m[2][2] - m[0][0] - m[1][1]) * 2; q[3] = (m[1][0] - m[0][1]) / s; q[0] = (m[0][2] + m[2][0]) / s; q[1] = (m[1][2] + m[2][1]) / s; q[2] = .25f * s; }
    VRIK_QuatNorm(q);
}

static hm::Mesh handMeshes[2];
static std::vector<size_t> padVertices[2][3];
static std::vector<uint8_t> skinUsed[2];
static std::vector<size_t> thumbVertices[2];   // the drawn thumb's skin

static void ResetSides() {
    ResetBuffer();
    for (auto& side : g_side) { side.active = false; side.blend = 0; side.palm = false; side.rev = 0; side.solved = ~0u;
        side.person = side.personOn = side.shownOn = false; side.style = 0; side.req = Request{}; }
    cvr::anim::GraspFingers(g_buf.data());
}
// The palm's axes as one rotation (hand frame): d, n and d x n, the same construction the game uses for baked seats.
static void PalmBasis(const Palm& pm, float* q) {
    float d[3]{pm.d[0], pm.d[1], pm.d[2]}, n[3], t[3];
    Unit(d);
    const float along = Dot(pm.n, d);
    for (int k = 0; k < 3; ++k) n[k] = pm.n[k] - d[k] * along;
    Unit(n); Cross(d, n, t);
    const float m[3][3]{{d[0], n[0], t[0]}, {d[1], n[1], t[1]}, {d[2], n[2], t[2]}};
    const float tr = m[0][0] + m[1][1] + m[2][2];
    if (tr > 0) { const float s = std::sqrt(tr + 1) * 2; q[3] = .25f * s; q[0] = (m[2][1] - m[1][2]) / s; q[1] = (m[0][2] - m[2][0]) / s; q[2] = (m[1][0] - m[0][1]) / s; }
    else if (m[0][0] > m[1][1] && m[0][0] > m[2][2]) { const float s = std::sqrt(1 + m[0][0] - m[1][1] - m[2][2]) * 2; q[3] = (m[2][1] - m[1][2]) / s; q[0] = .25f * s; q[1] = (m[0][1] + m[1][0]) / s; q[2] = (m[0][2] + m[2][0]) / s; }
    else if (m[1][1] > m[2][2]) { const float s = std::sqrt(1 + m[1][1] - m[0][0] - m[2][2]) * 2; q[3] = (m[0][2] - m[2][0]) / s; q[0] = (m[0][1] + m[1][0]) / s; q[1] = .25f * s; q[2] = (m[1][2] + m[2][1]) / s; }
    else { const float s = std::sqrt(1 + m[2][2] - m[0][0] - m[1][1]) * 2; q[3] = (m[1][0] - m[0][1]) / s; q[0] = (m[0][2] + m[2][0]) / s; q[1] = (m[1][2] + m[2][1]) / s; q[2] = .25f * s; }
    VRIK_QuatNorm(q);
}
static float AngleBetween(const float* a, const float* b) {
    const float dot = std::fabs(a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]);
    return 2 * std::acos(std::min(1.f, dot)) / kDeg;
}

struct Fit {
    Pose seat;            // slot frame, as Seat gave it
    int kind = -1;
    float d = 0, n = 0, t = 0, tilt = 0;
    float rotation[kSlots][4]{};
    float clearance[6]{};
    float padGap[3]{1, 1, 1};
    float angles[17]{}, thumb[4]{};
    Pose placed;          // hand frame, after the shift and the tilt
    double score = 1e30;
    bool refined = false; float thumbSwing[3]{};   // RefineThumb: the thumb moved onto the drawn surface, degrees per joint
};
static Fit Evaluate(int side, const Pose& seat, int kind, const Target& target, const Surface& surface, float d, float n, float t, float tilt) {
    ResetSides();
    auto& state = g_side[side];
    Fit fit; fit.seat = seat; fit.kind = kind; fit.d = d; fit.n = n; fit.t = t; fit.tilt = tilt;
    Pose placed = Compose(state.pm.slot, seat);
    const float* axes[3]{state.pm.d, state.pm.n, state.pm.t};
    const float offsets[3]{d, n, t};
    for (int a = 0; a < 3; ++a) Mad(placed.p, axes[a], offsets[a], placed.p);
    if (tilt != 0) { float turn[4], r[4]; AxisAngle(state.pm.n, tilt * kDeg, turn); VRIK_QuatMul(turn, placed.q, r); std::copy_n(r, 4, placed.q); VRIK_QuatNorm(placed.q); }
    fit.placed = placed;
    Request request; request.shape = target.shape; std::copy_n(target.half, 3, request.half);
    request.ends = target.ends; request.part = target.part; request.lathe = target.lathe;
    ToFrame(state.pm.slot, placed.p, request.obj.p);
    float inverse[4]; VRIK_QuatConj(state.pm.slot.q, inverse); VRIK_QuatMul(inverse, placed.q, request.obj.q); VRIK_QuatNorm(request.obj.q);
    state.req = request; state.active = true; state.blend = 1; ++state.rev;
    cvr::anim::GraspFingers(g_buf.data());
    std::memcpy(fit.rotation, state.rot, sizeof(fit.rotation));
    for (int i = 0; i < 17; ++i) fit.angles[i] = CyberpunkVR_DebugGraspAngles[side][i];
    for (int i = 0; i < 4; ++i) fit.thumb[i] = CyberpunkVR_DebugGraspThumb[side][i];
    const int hand = side ? g_VRRightBoneIdx : g_VRLeftBoneIdx;
    hm::Skin(handMeshes[side], hand);
    const Pose obj = Compose(state.pm.slot, state.req.obj);
    const auto& mesh = handMeshes[side];
    for (float& c : fit.clearance) c = 1e9f;
    for (size_t k = 0; k < mesh.sv.size() / 3; ++k) {
        if (!skinUsed[side][k]) continue;
        float l[3]; ToFrame(obj, &mesh.sv[k * 3], l);
        const float dist = SurfaceDistance(l, const_cast<Surface*>(&surface));
        if (dist < fit.clearance[mesh.group[k]]) fit.clearance[mesh.group[k]] = dist;
    }
    for (int digit = 0; digit < 3; ++digit) {
        float best = 1e9f;
        for (size_t v : padVertices[side][digit]) { float l[3]; ToFrame(obj, &mesh.sv[v * 3], l); best = std::min(best, SurfaceDistance(l, const_cast<Surface*>(&surface))); }
        fit.padGap[digit] = best;
    }
    const bool precision = kind == kPinch || kind == kTripod;
    double score = 300 * (double(d) * d + double(n) * n + double(t) * t) + .0003 * tilt * tilt;
    for (int g = 0; g < 6; ++g) {
        const double pen = std::max(0.f, -fit.clearance[g] - .001f); score += 1e6 * pen * pen;
        if (g >= 1 && (!precision || g <= 3)) { const double gap = std::max(0.f, fit.clearance[g] - .004f); score += 2e4 * gap * gap; }
    }
    if (precision) for (float gap : fit.padGap) { const double g = std::max(0.f, gap - .003f); score += 8e4 * g * g; }
    const double pad = std::max(0.f, fit.thumb[2] / 1000 - .004f); score += 2e4 * pad * pad;
    const double goal = std::max(0.f, fit.thumb[3] - 35.f) / 1000; score += 4000 * goal * goal;
    const float lower[4]{-5, -25, 0, -5}, upper[4]{70, 65, 60, 80};
    for (int j = 0; j < 4; ++j) { const float a = fit.angles[j + 1]; const float over = std::max({0.f, lower[j] - a, a - upper[j]}); score += .2 * over * over; }
    fit.score = score;
    return fit;
}
static bool Accept(const Fit& f) {
    for (float c : f.clearance) if (!std::isfinite(c) || c < -.0015f) return false;
    if (f.kind == kTripod || f.kind == kPinch) { for (float g : f.padGap) if (!(g <= .004f)) return false; }
    else {
        // a grasp holds: the palm or the thumb on it, and two fingers
        int fingers = 0;
        for (int g = 2; g < 6; ++g) fingers += f.clearance[g] <= .005f;
        if (!(f.clearance[0] <= .005f || f.clearance[1] <= .005f) || fingers < 2) return false;
    }
    for (int finger = 0; finger < 4; ++finger) if (f.angles[7 + 3 * finger] > .8f * f.angles[6 + 3 * finger] + 15.1f) return false;
    const float lower[4]{-5, -25, 0, -5}, upper[4]{70, 65, 60, 80};
    for (int j = 0; j < 4; ++j) if (f.angles[j + 1] < lower[j] - .1f || f.angles[j + 1] > upper[j] + .1f) return false;
    return true;
}
static Fit Search(int side, const Pose& seat, int kind, const Target& half, const Surface& surface) {
    Fit best = Evaluate(side, seat, kind, half, surface, 0, 0, 0, 0);
    for (float d : {-.012f, 0.f, .012f}) for (float n : {-.008f, 0.f, .008f, .016f}) for (float t : {-.015f, 0.f, .015f}) for (float tilt : {-12.f, 0.f, 12.f}) {
        if (d == 0 && n == 0 && t == 0 && tilt == 0) continue;
        const Fit f = Evaluate(side, seat, kind, half, surface, d, n, t, tilt);
        if (f.score < best.score) best = f;
    }
    for (float step : {.004f, .002f}) for (int it = 0; it < 2; ++it) {
        bool improved = false;
        for (int dim = 0; dim < 3; ++dim) for (float sign : {-1.f, 1.f}) {
            float s[3]{best.d, best.n, best.t}; s[dim] = std::clamp(s[dim] + sign * step, -.03f, .03f);
            const Fit f = Evaluate(side, seat, kind, half, surface, s[0], s[1], s[2], best.tilt);
            if (f.score < best.score) { best = f; improved = true; }
        }
        if (!improved) break;
    }
    return best;
}
// THE THUMB ON THE DRAWN SURFACE. The solver closes the thumb as capsules round its bones on the surface; the drawn thumb
// is not those capsules, so its pad often stops short of what it holds or its side sinks in. Here each of its three bones
// swings (never twists: the axes square to the bone) a little off where the solver left it, by coordinate descent, until
// the drawn pad rests on the drawn surface (1 mm) with no thumb skin in it -- at most kThumbSwing off at each joint, the
// sum of the swings kept small so the solver's anatomy (opposition, roll, the joints' ranges) stays.
constexpr float kThumbSwing = 16.f * kDeg;
struct ThumbState { float clearance, pad; };
static ThumbState MeasureThumb(int side, const Pose& obj, const Surface& surface) {
    const auto& mesh = handMeshes[side];
    ThumbState out{1e9f, 1e9f};
    for (size_t v : thumbVertices[side]) { float l[3]; ToFrame(obj, &mesh.sv[v * 3], l); out.clearance = std::min(out.clearance, SurfaceDistance(l, const_cast<Surface*>(&surface))); }
    for (size_t v : padVertices[side][0]) { float l[3]; ToFrame(obj, &mesh.sv[v * 3], l); out.pad = std::min(out.pad, SurfaceDistance(l, const_cast<Surface*>(&surface))); }
    return out;
}
static double ThumbCost(const ThumbState& m, const float (*swing)[2]) {
    double cost = 0;
    const double pen = std::max(0.f, -m.clearance - .0005f); cost += 1e6 * pen * pen;
    const double gap = std::max(0.f, m.pad - .001f); cost += 4e4 * gap * gap;
    for (int j = 0; j < 3; ++j) cost += .02 * (double(swing[j][0]) * swing[j][0] + double(swing[j][1]) * swing[j][1]);
    return cost;
}
static void WriteFingers(int side, const float (*rotation)[4]) {
    for (int slot = 0; slot < kSlots; ++slot) {
        const int bone = VRIK_LadderFingerBone(side, slot);
        if (!ValidBone(bone)) continue;
        std::memcpy(g_buf.data() + bone * 48 + VRIK_ROT_OFF, rotation[slot], 16);
    }
}
static bool RefineThumb(int side, Fit& fit, const Target& target, const Surface& surface) {
    // the fit's pose in the buffer, skinned
    (void)Evaluate(side, fit.seat, fit.kind, target, surface, fit.d, fit.n, fit.t, fit.tilt);
    const int hand = side ? g_VRRightBoneIdx : g_VRLeftBoneIdx;
    const Pose obj = fit.placed;
    int bones[3]; float axes[3][2][3];
    for (int j = 0; j < 3; ++j) {
        bones[j] = VRIK_LadderFingerBone(side, kThumbSlots[j]);
        if (!ValidBone(bones[j])) return false;
    }
    for (int j = 0; j < 3; ++j) {
        float dir[3];
        if (j < 2) std::copy_n(Local(g_buf.data(), bones[j + 1]).p, 3, dir); else TipOffset(g_buf.data(), bones[2], dir);
        if (!Unit(dir)) return false;
        const float other[3]{std::fabs(dir[0]) < .9f ? 1.f : 0.f, std::fabs(dir[0]) < .9f ? 0.f : 1.f, 0.f};
        Cross(dir, other, axes[j][0]); Unit(axes[j][0]); Cross(dir, axes[j][0], axes[j][1]); Unit(axes[j][1]);
    }
    float base[3][4]; for (int j = 0; j < 3; ++j) std::copy_n(fit.rotation[kThumbSlots[j]], 4, base[j]);
    float swing[3][2]{};
    float rotation[kSlots][4]; std::memcpy(rotation, fit.rotation, sizeof(rotation));
    const auto apply = [&](const float (*sw)[2]) {
        for (int j = 0; j < 3; ++j) {
            float a[4], b[4], q[4], r[4];
            AxisAngle(axes[j][0], sw[j][0], a); AxisAngle(axes[j][1], sw[j][1], b);
            VRIK_QuatMul(a, b, q); VRIK_QuatMul(base[j], q, r); VRIK_QuatNorm(r);
            std::copy_n(r, 4, rotation[kThumbSlots[j]]);
        }
        WriteFingers(side, rotation);
        hm::Skin(handMeshes[side], hand);
        return MeasureThumb(side, obj, surface);
    };
    ThumbState now = apply(swing);
    double cost = ThumbCost(now, swing);
    const double start = cost;
    for (float step : {6.f * kDeg, 3.f * kDeg, 1.5f * kDeg, .75f * kDeg})
        for (int round = 0; round < 4; ++round) {
            bool improved = false;
            for (int j = 0; j < 3; ++j) for (int a = 0; a < 2; ++a) for (float sign : {-1.f, 1.f}) {
                float trial[3][2]; std::memcpy(trial, swing, sizeof(trial));
                trial[j][a] += sign * step;
                if (std::hypot(trial[j][0], trial[j][1]) > kThumbSwing) continue;
                const ThumbState m = apply(trial);
                const double c = ThumbCost(m, trial);
                if (c < cost - 1e-9) { cost = c; now = m; std::memcpy(swing, trial, sizeof(swing)); improved = true; }
            }
            if (!improved) break;
        }
    // the best swings in the buffer and the fit, every group measured again
    (void)apply(swing);
    if (!(cost < start)) { WriteFingers(side, fit.rotation); hm::Skin(handMeshes[side], hand); return false; }
    std::memcpy(fit.rotation, rotation, sizeof(rotation));
    const auto& mesh = handMeshes[side];
    for (float& c : fit.clearance) c = 1e9f;
    for (size_t k = 0; k < mesh.sv.size() / 3; ++k) {
        if (!skinUsed[side][k]) continue;
        float l[3]; ToFrame(obj, &mesh.sv[k * 3], l);
        const float dist = SurfaceDistance(l, const_cast<Surface*>(&surface));
        if (dist < fit.clearance[mesh.group[k]]) fit.clearance[mesh.group[k]] = dist;
    }
    for (int digit = 0; digit < 3; ++digit) {
        float best = 1e9f;
        for (size_t v : padVertices[side][digit]) { float l[3]; ToFrame(obj, &mesh.sv[v * 3], l); best = std::min(best, SurfaceDistance(l, const_cast<Surface*>(&surface))); }
        fit.padGap[digit] = best;
    }
    for (int j = 0; j < 3; ++j) fit.thumbSwing[j] = std::hypot(swing[j][0], swing[j][1]) / kDeg;
    fit.refined = true;
    return true;
}
static void Preview(int side, const Fit& fit, const Target& half, const Surface& surface, const std::filesystem::path& path) {
    (void)Evaluate(side, fit.seat, fit.kind, half, surface, fit.d, fit.n, fit.t, fit.tilt);
    if (fit.refined) { WriteFingers(side, fit.rotation); hm::Skin(handMeshes[side], side ? g_VRRightBoneIdx : g_VRLeftBoneIdx); }
    auto& state = g_side[side];
    Scene scene; scene.pm = state.pm; scene.rq = state.req; scene.obj = Compose(state.pm.slot, state.req.obj); scene.mesh = &handMeshes[side];
    const char* sized = std::getenv("GRIP_PREVIEW_SIZE");
    const int size = sized ? std::clamp(std::atoi(sized), 100, 800) : 200; std::vector<uint8_t> pixels(size_t(size) * size * 3 * 3);
    float centre[3]; Mad(scene.pm.skin, scene.pm.d, .045f, centre); Mad(centre, scene.pm.n, .025f, centre);
    const float views[3][3]{{1, 0, 0}, {.5f, .6f, .6f}, {0, 0, -1}};
    for (int v = 0; v < 3; ++v) {
        float fwd[3]; for (int k = 0; k < 3; ++k) fwd[k] = scene.pm.t[k] * views[v][0] + scene.pm.d[k] * views[v][1] + scene.pm.n[k] * views[v][2];
        Unit(fwd); Render(scene, centre, .34f, size, pixels, v * size, 3 * size, fwd, scene.pm.d);
    }
    std::ofstream file(path, std::ios::binary); Need(bool(file), "cannot write preview");
    file << "P6\n" << 3 * size << ' ' << size << "\n255\n"; file.write(reinterpret_cast<const char*>(pixels.data()), std::streamsize(pixels.size()));
}

static void PrepareHands(const char* rig) {
    g_buf.assign(800 * 48, 0);
    for (int b = 0; b < 800; ++b) reinterpret_cast<float*>(&g_buf[b * 48 + VRIK_ROT_OFF])[3] = 1;
    LoadRig(rig);
    const auto directory = std::filesystem::path(rig).parent_path();
    hm::LoadNames((directory / "mesh/rig_names.txt").string().c_str()); ResetBuffer();
    Need(hm::Load(handMeshes[0], (directory / "mesh/hand_l.bin").string().c_str(), 0, g_VRLeftBoneIdx), "left hand mesh unavailable");
    Need(hm::Load(handMeshes[1], (directory / "mesh/hand_r.bin").string().c_str(), 1, g_VRRightBoneIdx), "right hand mesh unavailable");
    ResetSides();
    for (int side = 0; side < 2; ++side) {
        const auto& mesh = handMeshes[side];
        hm::Skin(handMeshes[side], side ? g_VRRightBoneIdx : g_VRLeftBoneIdx);
        skinUsed[side].assign(mesh.sv.size() / 3, 0);
        for (size_t t = 0; t < mesh.draw.size(); ++t)
            if (mesh.draw[t]) skinUsed[side][mesh.tri[t * 3]] = skinUsed[side][mesh.tri[t * 3 + 1]] = skinUsed[side][mesh.tri[t * 3 + 2]] = 1;
        for (int digit = 0; digit < 3; ++digit) {
            const int* slots = digit == 0 ? kThumbSlots : kFingers[digit - 1].slot;
            const int distal = s_fingers[side][slots[2]], hand = side ? g_VRRightBoneIdx : g_VRLeftBoneIdx;
            float reference[kSlots][4]; ReferenceFingers(g_buf.data(), side, reference, nullptr);
            ChainBones chain; Need(ChainOf(g_buf.data(), side, hand, slots, reference, chain), "finger reference chain unavailable");
            float rotations[3][4], joints[4][3], hinge[3][3]; Pose frames[3];
            for (int k = 0; k < 3; ++k) std::copy_n(chain.local[k].q, 4, rotations[k]);
            Joints(chain, rotations, joints, frames);
            float toward[3]; std::copy_n(g_side[side].pm.n, 3, toward);
            if (digit == 0) Sub(g_side[side].pm.skin, joints[1], toward);
            Need(Hinges(joints, frames, toward, hinge), "finger reference hinge unavailable");
            float segment[3], axis[3], padNormal[3]; Sub(joints[3], joints[2], segment); Rot(frames[2].q, hinge[2], axis); Cross(axis, segment, padNormal); Unit(padNormal);
            for (size_t vertex = 0; vertex < mesh.group.size(); ++vertex) {
                if (mesh.group[vertex] != digit + 1) continue;
                float influence = 0;
                for (int slot = 0; slot < 8; ++slot) {
                    const auto joint = mesh.j[vertex * 8 + slot];
                    const float share = (mesh.a[joint] == distal ? 1 - mesh.mix[joint] : 0) + (mesh.b[joint] == distal ? mesh.mix[joint] : 0);
                    influence += mesh.w[vertex * 8 + slot] * share;
                }
                if (influence >= .4f && Dot(&mesh.sn[vertex * 3], padNormal) > .2f) padVertices[side][digit].push_back(vertex);
            }
            Need(!padVertices[side][digit].empty(), "finger pad skin unavailable");
        }
        for (size_t vertex = 0; vertex < mesh.group.size(); ++vertex)
            if (mesh.group[vertex] == 1 && skinUsed[side][vertex]) thumbVertices[side].push_back(vertex);
        Need(!thumbVertices[side].empty(), "thumb skin unavailable");
    }
}

static Json Quats(const float (*q)[4], int n) {
    Json out = Json::array();
    for (int i = 0; i < n; ++i) out.push_back({q[i][0], q[i][1], q[i][2], q[i][3]});
    return out;
}
static Json BakeMesh(const Json& row, gripgen::Oodle& oodle, const std::filesystem::path& previewDir, bool preview) {
    gripgen::Cr2w cooked(row.at("source").get<std::string>());
    const auto mesh = gripgen::DecodeMesh(cooked, oodle);
    float lo[3]{1e9f, 1e9f, 1e9f}, hi[3]{-1e9f, -1e9f, -1e9f};
    for (const auto& v : mesh.vertices) {
        const float p[3]{v.x, v.y, v.z};
        for (int k = 0; k < 3; ++k) lo[k] = std::min(lo[k], p[k]), hi[k] = std::max(hi[k], p[k]);
    }
    float centre[3], half[3];
    for (int k = 0; k < 3; ++k) centre[k] = .5f * (lo[k] + hi[k]), half[k] = std::max(.002f, .5f * (hi[k] - lo[k]));
    const Field field = BuildField(mesh, lo, hi);
    // the mesh's hash: the game's seat takes its fitted shift by it (GraspSeatFits.inc), so the base seat here is the game's
    uint64_t meshHash = 0;
    { const auto text = row.at("hash").get<std::string>(); std::from_chars(text.data(), text.data() + text.size(), meshHash, 16); }
    std::vector<Target> targets;
    int roundAxis = -1;
    if (row.contains("round_profile") && row.at("round_profile").is_object()) {
        // a round mesh: as HeldBody takes it -- the cylinder round the whole of it as thick as it is drawn, and each part
        // GraspPartOf finds where a palm could be along it, every one with the whole drawn round it
        const Json& rp = row.at("round_profile");
        const int a = rp.at("axis").get<int>(), b = (a + 1) % 3, c = (a + 2) % 3;
        const float low = rp.at("low").get<float>(), high = rp.at("high").get<float>();
        float radii[16], widest = 0;
        for (int k = 0; k < 16; ++k) widest = std::max(widest, radii[k] = rp.at("radii").at(k).get<float>());
        if (a >= 0 && a < 3 && high - low > .01f && widest > .004f) {
            roundAxis = a;
            Pose cylinder;     // Z on the mesh's axis, X and Y on the next two (a turn, not a mirror), at the profile's middle
            float m[3][3]{}; m[b][0] = 1; m[c][1] = 1; m[a][2] = 1; MatrixQuat(m, cylinder.q);
            cylinder.p[a] = .5f * (low + high); cylinder.p[b] = rp.at("center_b").get<float>(); cylinder.p[c] = rp.at("center_c").get<float>();
            const float halfLength = .5f * (high - low), w = (high - low) / 16;
            std::vector<float> ring;
            for (int k = 0; k < 16; ++k) for (int r = 0; r < 8; ++r) {
                const float angle = 6.2831853f * r / 8;
                ring.insert(ring.end(), {radii[k] * std::cos(angle), radii[k] * std::sin(angle), low + w * (k + .5f) - cylinder.p[a]});
            }
            const int count = int(ring.size() / 3);
            Target whole; whole.shape = kCylinder; whole.half[0] = whole.half[1] = widest; whole.half[2] = halfLength; whole.inMesh = cylinder;
            for (int k = 0; k < 16; ++k) {
                const float z = low + w * (k + .5f) - cylinder.p[a];
                const float hand[3]{radii[k] + .025f, 0, z};
                cvr::anim::GraspPart gp;
                const bool found = cvr::anim::GraspPartOf(ring.data(), count, halfLength, hand, gp);
                if (whole.lathe.knots < 2 && gp.lathe.knots >= 2) whole.lathe = gp.lathe;
                if (!found) continue;
                bool seenPart = false;
                for (const auto& t : targets) seenPart |= std::fabs(t.inMesh.p[a] - cylinder.p[a] - gp.z) < .01f && std::fabs(t.half[0] - gp.radius) < .003f;
                if (seenPart) continue;
                Target part; part.shape = kCylinder; part.half[0] = part.half[1] = gp.radius; part.half[2] = gp.half;
                part.inMesh = cylinder; const float along[3]{0, 0, gp.z}; float moved[3]; Rot(cylinder.q, along, moved);
                for (int i = 0; i < 3; ++i) part.inMesh.p[i] += moved[i];
                part.ends = gp.ends; part.part = true; part.neck = gp.neck; part.stem = gp.stem; part.lathe = gp.lathe;
                targets.push_back(part);
            }
            targets.insert(targets.begin(), whole);
        }
    }
    if (targets.empty()) { Target box; std::copy_n(half, 3, box.half); std::copy_n(centre, 3, box.inMesh.p); targets.push_back(box); }
    Json out{{"hash", row.at("hash")}, {"path", row.at("path")}, {"centre", {centre[0], centre[1], centre[2]}}, {"half", {half[0], half[1], half[2]}},
             {"cell_m", field.cell}, {"triangles", mesh.triangles.size()}, {"round_axis", roundAxis}, {"targets", Json::array()}, {"sides", Json::array()}};
    for (const auto& t : targets)
        out["targets"].push_back({{"shape", t.shape}, {"half", {t.half[0], t.half[1], t.half[2]}}, {"p", {t.inMesh.p[0], t.inMesh.p[1], t.inMesh.p[2]}},
                                  {"q", {t.inMesh.q[0], t.inMesh.q[1], t.inMesh.q[2], t.inMesh.q[3]}}, {"ends", t.ends}, {"part", t.part},
                                  {"neck", t.neck}, {"stem", t.stem}, {"lathe_knots", t.lathe.knots}});
    // the 24 turns that keep the box's axes on the palm's
    std::vector<std::array<int, 3>> perms{{0, 1, 2}, {0, 2, 1}, {1, 0, 2}, {1, 2, 0}, {2, 0, 1}, {2, 1, 0}};
    for (int side = 0; side < 2; ++side) {
        ResetSides();
        const Palm pm = g_side[side].pm;
        float basis[4]; PalmBasis(pm, basis);
        float basisInv[4]; VRIK_QuatConj(basis, basisInv);
        struct Seen { int target, kind; float rel[4]; float p[3]; };
        std::vector<Seen> seen;
        Json grasps = Json::array();
        Fit bestShown; bool shown = false; int shownTarget = 0;
        for (int ti = 0; ti < int(targets.size()); ++ti) {
        const Target& target = targets[size_t(ti)];
        Surface surface{&field, target.inMesh};
        bool axisDone[6]{};
        for (const auto& perm : perms)
            for (int signs = 0; signs < 8; ++signs) {
                // columns: where the box's x, y, z go, in the palm's (d, n, d x n)
                float m[3][3]{};
                for (int axis = 0; axis < 3; ++axis) m[perm[axis]][axis] = (signs >> axis & 1) ? -1.f : 1.f;
                const float det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
                if (det < 0) continue;
                if (target.shape == kCylinder) {
                    // round: only where its axis goes matters (the seat turns it about its axis as the palm needs)
                    const int to = perm[2] * 2 + (signs >> 2 & 1);
                    if (axisDone[to]) continue;
                    axisDone[to] = true;
                }
                // the turn, in the palm's frame, then in the hand's
                float r[4]; {
                    float q[4]; const float tr = m[0][0] + m[1][1] + m[2][2];
                    if (tr > 0) { const float s = std::sqrt(tr + 1) * 2; q[3] = .25f * s; q[0] = (m[2][1] - m[1][2]) / s; q[1] = (m[0][2] - m[2][0]) / s; q[2] = (m[1][0] - m[0][1]) / s; }
                    else if (m[0][0] > m[1][1] && m[0][0] > m[2][2]) { const float s = std::sqrt(1 + m[0][0] - m[1][1] - m[2][2]) * 2; q[3] = (m[2][1] - m[1][2]) / s; q[0] = .25f * s; q[1] = (m[0][1] + m[1][0]) / s; q[2] = (m[0][2] + m[2][0]) / s; }
                    else if (m[1][1] > m[2][2]) { const float s = std::sqrt(1 + m[1][1] - m[0][0] - m[2][2]) * 2; q[3] = (m[0][2] - m[2][0]) / s; q[0] = (m[0][1] + m[1][0]) / s; q[1] = .25f * s; q[2] = (m[1][2] + m[2][1]) / s; }
                    else { const float s = std::sqrt(1 + m[2][2] - m[0][0] - m[1][1]) * 2; q[3] = (m[1][0] - m[0][1]) / s; q[0] = (m[0][2] + m[2][0]) / s; q[1] = (m[1][2] + m[2][1]) / s; q[2] = .25f * s; }
                    VRIK_QuatNorm(q); VRIK_QuatMul(basis, q, r); VRIK_QuatNorm(r);
                }
                // the box resting on the palm's middle, then where the production seat puts it
                Pose inHand; std::copy_n(r, 4, inHand.q);
                float nLocal[3]; Unrot(r, pm.n, nLocal);
                const float* th = target.half;
                const float support = th[0] * std::fabs(nLocal[0]) + th[1] * std::fabs(nLocal[1]) + th[2] * std::fabs(nLocal[2]);
                Mad(pm.skin, pm.n, support + .004f, inHand.p);
                Pose inSlot; float inv[4];
                ToFrame(pm.slot, inHand.p, inSlot.p); VRIK_QuatConj(pm.slot.q, inv); VRIK_QuatMul(inv, inHand.q, inSlot.q); VRIK_QuatNorm(inSlot.q);
                Pose seat; int kind = -1;
                ResetSides();
                if (!Seat(side, target.shape, target.half, inSlot.p, inSlot.q, nullptr, seat, &kind, target.ends, target.part,
                          target.lathe.knots >= 2 ? &target.lathe : nullptr, meshHash)) continue;
                const Pose seatHand = Compose(g_side[side].pm.slot, seat);
                float rel0[4]; VRIK_QuatMul(basisInv, seatHand.q, rel0); VRIK_QuatNorm(rel0);
                bool duplicate = false;
                for (const auto& s : seen) {
                    float dp[3]; Sub(s.p, seatHand.p, dp);
                    if (s.target == ti && s.kind == kind && AngleBetween(s.rel, rel0) < 10.f && Len(dp) < .005f) { duplicate = true; break; }
                }
                if (duplicate) continue;
                Seen record{ti, kind, {rel0[0], rel0[1], rel0[2], rel0[3]}, {seatHand.p[0], seatHand.p[1], seatHand.p[2]}};
                seen.push_back(record);
                g_graspSurfaceDistance = SurfaceDistance; g_graspSurfaceContext = &surface;
                Fit best = Search(side, seat, kind, target, surface);
                const bool before = Accept(best);
                Fit refined = best;
                if (RefineThumb(side, refined, target, surface) && (Accept(refined) || !before)) best = refined;
                const bool accepted = Accept(best);
                float rel[4]; VRIK_QuatMul(basisInv, best.placed.q, rel); VRIK_QuatNorm(rel);
                float offset[3]; Sub(best.placed.p, pm.skin, offset);
                // the mesh's own pose for it: the primitive's pose in the mesh taken off
                Pose meshInHand; { float inv[4]; VRIK_QuatConj(target.inMesh.q, inv); VRIK_QuatMul(best.placed.q, inv, meshInHand.q); VRIK_QuatNorm(meshInHand.q);
                    float back[3]; Rot(meshInHand.q, target.inMesh.p, back); Sub(best.placed.p, back, meshInHand.p); }
                float meshRel[4]; VRIK_QuatMul(basisInv, meshInHand.q, meshRel); VRIK_QuatNorm(meshRel);
                float meshOffset[3]; Sub(meshInHand.p, pm.skin, meshOffset);
                float dUnit[3]{pm.d[0], pm.d[1], pm.d[2]}; Unit(dUnit);
                float nUnit[3]; { const float a = Dot(pm.n, dUnit); for (int k = 0; k < 3; ++k) nUnit[k] = pm.n[k] - dUnit[k] * a; Unit(nUnit); }
                float tUnit[3]; Cross(dUnit, nUnit, tUnit);
                float delta[kSlots][4];
                for (int slot = 0; slot < kSlots; ++slot) {
                    const int bone = s_fingers[side][slot]; float refInv[4]; VRIK_QuatConj(s_ref[bone], refInv);
                    VRIK_QuatMul(refInv, best.rotation[slot], delta[slot]); VRIK_QuatNorm(delta[slot]);
                }
                grasps.push_back({{"target", ti}, {"kind", kind}, {"accepted", accepted}, {"score", best.score},
                                  {"mesh_rel", {meshRel[0], meshRel[1], meshRel[2], meshRel[3]}},
                                  {"clearance_m", best.clearance}, {"pad_gap_m", best.padGap}, {"thumb", best.thumb},
                                  {"thumb_swing_deg", best.thumbSwing}, {"refined", best.refined}, {"accepted_unrefined", before},
                                  {"shift_dnt_m", {best.d, best.n, best.t}}, {"tilt_deg", best.tilt},
                                  {"seat_rel", {rel0[0], rel0[1], rel0[2], rel0[3]}},
                                  {"pose_pos_dnt_m", {Dot(offset, dUnit), Dot(offset, nUnit), Dot(offset, tUnit)}},
                                  {"pose_rel", {rel[0], rel[1], rel[2], rel[3]}},
                                  {"mesh_pos_dnt_m", {Dot(meshOffset, dUnit), Dot(meshOffset, nUnit), Dot(meshOffset, tUnit)}},
                                  {"palm_half_dt_m", {pm.halfD, pm.halfT}},
                                  {"finger_deltas", Quats(delta, kSlots)}});
                // GRIP_PREVIEW_TARGET / GRIP_PREVIEW_KIND: show that primitive's (or kind's) best grasp instead of the mesh's best
                const char* wantT = std::getenv("GRIP_PREVIEW_TARGET"); const char* wantK = std::getenv("GRIP_PREVIEW_KIND");
                const char* wantSign = std::getenv("GRIP_PREVIEW_TSIGN");   // the mesh's origin on that side of the palm (d x n)
                const bool wanted = (!wantT || std::atoi(wantT) == ti) && (!wantK || std::atoi(wantK) == kind) &&
                                    (!wantSign || Dot(meshOffset, tUnit) * std::atoi(wantSign) > 0);
                if (accepted && wanted && (!shown || best.score < bestShown.score)) { bestShown = best; shown = true; shownTarget = ti; }
                g_graspSurfaceDistance = nullptr; g_graspSurfaceContext = nullptr;
            }
        }
        if (preview && shown) {
            Surface surface{&field, targets[size_t(shownTarget)].inMesh};
            g_graspSurfaceDistance = SurfaceDistance; g_graspSurfaceContext = &surface;
            Preview(side, bestShown, targets[size_t(shownTarget)], surface, previewDir / (row.at("hash").get<std::string>() + (side ? "_r.ppm" : "_l.ppm")));
            g_graspSurfaceDistance = nullptr; g_graspSurfaceContext = nullptr;
        }
        out["sides"].push_back({{"side", side}, {"grasps", grasps}});
    }
    return out;
}

// ---- the runtime table: one file, little-endian, sorted by mesh hash (src/Anim/GraspPose.cpp reads it) ----
// A mesh: its grasps and its primitives (targets). A grasp: the hand, the kind, its primitive, the palm it was fitted for,
// the MESH's pose against the palm (position along d, n, d x n; turn in the palm's basis) and each finger slot's turn off
// the rig's reference (x, y, z of a unit quaternion with w >= 0, by 32767).
#pragma pack(push, 1)
struct FileHeader { uint32_t magic, version, meshes, targets, grasps; };
struct FileMesh { uint64_t hash; uint32_t firstGrasp, firstTarget; uint16_t grasps; uint8_t targets; int8_t roundAxis; };
struct FileTarget { uint8_t shape, ends, part, pad; float half[3], p[3], q[4]; };
struct FileGrasp { uint8_t side, kind, target, pad; float palmD, palmT, pos[3], rel[4]; int16_t delta[kSlots][3]; };
#pragma pack(pop)
static_assert(sizeof(FileHeader) == 20 && sizeof(FileMesh) == 20 && sizeof(FileTarget) == 44 && sizeof(FileGrasp) == 4 + 8 + 12 + 16 + kSlots * 6);
static int Pack(const char* output, int count, char** inputs) {
    std::map<uint64_t, Json> meshes;
    size_t read = 0, failed = 0;
    for (int i = 0; i < count; ++i) {
        std::ifstream in(inputs[i]); Need(bool(in), "cannot open baked shard");
        std::string line;
        while (std::getline(in, line)) {
            if (line.empty()) continue;
            Json row = Json::parse(line); ++read;
            if (row.value("status", std::string("ok")) != "ok") { ++failed; continue; }
            const auto text = row.at("hash").get<std::string>(); uint64_t hash{};
            const auto parsed = std::from_chars(text.data(), text.data() + text.size(), hash, 16);
            Need(parsed.ec == std::errc{} && parsed.ptr == text.data() + text.size(), "invalid mesh hash");
            meshes[hash] = std::move(row);
        }
    }
    std::vector<FileMesh> table; std::vector<FileTarget> targets; std::vector<FileGrasp> grasps;
    for (const auto& [hash, row] : meshes) {
        if (!row.contains("targets")) continue;
        FileMesh m{}; m.hash = hash; m.firstGrasp = uint32_t(grasps.size()); m.firstTarget = uint32_t(targets.size());
        m.roundAxis = int8_t(row.value("round_axis", -1));
        Need(row.at("targets").size() < 250, "too many targets");
        for (const auto& t : row.at("targets")) {
            FileTarget f{}; f.shape = uint8_t(t.at("shape").get<int>()); f.ends = uint8_t(t.at("ends").get<int>()); f.part = t.at("part").get<bool>();
            for (int k = 0; k < 3; ++k) { f.half[k] = t.at("half").at(k).get<float>(); f.p[k] = t.at("p").at(k).get<float>(); }
            for (int k = 0; k < 4; ++k) f.q[k] = t.at("q").at(k).get<float>();
            targets.push_back(f); ++m.targets;
        }
        for (const auto& side : row.at("sides"))
            for (const auto& g : side.at("grasps")) {
                if (!g.at("accepted").get<bool>()) continue;
                FileGrasp f{}; f.side = uint8_t(side.at("side").get<int>()); f.kind = uint8_t(g.at("kind").get<int>()); f.target = uint8_t(g.at("target").get<int>());
                f.palmD = g.at("palm_half_dt_m").at(0).get<float>(); f.palmT = g.at("palm_half_dt_m").at(1).get<float>();
                for (int k = 0; k < 4; ++k) f.rel[k] = g.at("mesh_rel").at(k).get<float>();
                for (int k = 0; k < 3; ++k) f.pos[k] = g.at("mesh_pos_dnt_m").at(k).get<float>();
                for (int s = 0; s < kSlots; ++s) {
                    float q[4]; for (int k = 0; k < 4; ++k) q[k] = g.at("finger_deltas").at(s).at(k).get<float>();
                    VRIK_QuatNorm(q); const float sign = q[3] < 0 ? -1.f : 1.f;
                    for (int k = 0; k < 3; ++k) f.delta[s][k] = int16_t(std::lround(std::clamp(sign * q[k], -1.f, 1.f) * 32767.f));
                }
                grasps.push_back(f); ++m.grasps;
                Need(m.grasps < 65535, "too many grasps");
            }
        if (m.grasps) table.push_back(m);
        else targets.resize(m.firstTarget);
    }
    std::ofstream out(output, std::ios::binary); Need(bool(out), "cannot write grasp table");
    const FileHeader header{0x47525643u /* CVRG */, 2, uint32_t(table.size()), uint32_t(targets.size()), uint32_t(grasps.size())};
    out.write(reinterpret_cast<const char*>(&header), sizeof(header));
    out.write(reinterpret_cast<const char*>(table.data()), std::streamsize(table.size() * sizeof(FileMesh)));
    out.write(reinterpret_cast<const char*>(targets.data()), std::streamsize(targets.size() * sizeof(FileTarget)));
    out.write(reinterpret_cast<const char*>(grasps.data()), std::streamsize(grasps.size() * sizeof(FileGrasp)));
    Need(bool(out), "grasp table write failed");
    std::cout << Json{{"rows", read}, {"failed_rows", failed}, {"meshes", table.size()}, {"grasps", grasps.size()}}.dump() << '\n';
    return 0;
}
}  // namespace batch

int main(int argc, char** argv) {
    using namespace batch;
    try {
        if (argc >= 4 && std::string_view(argv[1]) == "--pack") return Pack(argv[2], argc - 3, argv + 3);
        Need(argc >= 7, "usage: grip_pose_batch RIG OODLE CANDIDATES.jsonl OUT.jsonl SHARD SHARDS [PREVIEW_DIR PREVIEW_EVERY]");
        PrepareHands(argv[1]);
        gripgen::Oodle oodle(argv[2]);
        std::ifstream in(argv[3]); Need(bool(in), "cannot open candidates");
        std::ofstream out(argv[4], std::ios::app); Need(bool(out), "cannot open output");
        const int shard = std::stoi(argv[5]), shards = std::stoi(argv[6]);
        const std::filesystem::path previews = argc > 7 ? argv[7] : "";
        const int every = argc > 8 ? std::stoi(argv[8]) : 0;
        if (!previews.empty()) std::filesystem::create_directories(previews);
        std::string line; int index = -1, done = 0;
        const auto start = std::chrono::steady_clock::now();
        while (std::getline(in, line)) {
            if (line.empty()) continue;
            if (++index % shards != shard) continue;
            const Json row = Json::parse(line);
            Json result;
            try { result = BakeMesh(row, oodle, previews, every > 0 && index % every == 0); result["status"] = "ok"; }
            catch (const std::exception& e) { result = {{"hash", row.at("hash")}, {"path", row.at("path")}, {"status", "error"}, {"error", e.what()}}; }
            out << result.dump() << '\n' << std::flush;
            ++done;
            if (done % 10 == 0) std::cout << done << " meshes, " << std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count() << " s\n" << std::flush;
        }
        std::cout << "complete: " << done << " meshes\n";
        return 0;
    } catch (const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
}
