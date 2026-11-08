// ownshell: the port's own holster shells, made from the game's Valentino gang holster pieces (and the Maelstrom
// knife bandolier's sheath)
// (base\characters\garment\gang_valentino\item\i1_092_belt__harness). make_own_holsters.py does the file work; the
// geometry is here.
//
//   ownshell mirror  IN.bin OUT.bin                 the left piece made right: x -> -x, normals/tangents with it, the
//                                                   triangles turned round (tangent w flipped)
//   ownshell refit   FROM.bin TO.bin IN.bin OUT.bin  a garment made for one body carried onto another: each vertex keeps
//                                                   its offset from the nearest point of the FROM body, found again on
//                                                   the TO body along that point's normal, the displacement smoothed
//                                                   over the garment; `rigid` after OUT: moved as a whole
//   ownshell parts   IN.bin                         its connected pieces (by welded position): JSON, one line a piece
//   ownshell drop    IN.bin OUT.bin LIST             the triangles of the listed pieces ("prim:piece,...") removed
//   ownshell keep    IN.bin OUT.bin LIST             only the triangles of the listed pieces kept
//   ownshell pieces  IN.bin OUT.bin LIST             the points of the listed pieces only (float32 xyz, game axes)
//   ownshell fit2    HOLSTER.bin WEAPON.bin [opts]  the same by the holster's cavity (enclosure field): see CmdFit2
//   ownshell fitscale HOLSTER.bin WEAPON.bin OUT.bin [opts]  the holster stretched for the weapon, then fit2
//   ownshell fit     HOLSTER.bin WEAPON.bin [opts]  where a weapon sits in a holster: JSON pose (weapon frame -> the
//                                                   holster's model space, game axes)
//   ownshell place   BODY.bin FRAME.bin anchor= blade= gap= IN OUT ...   a rigid piece (a sheath) hung on the
//                                                   body: see CmdPlace
//   ownshell sheathfit SHEATH.bin KNIFE.xyz OUT.bin pose=...   a whole knife down the chest sheath, the sheath
//                                                   stretched where the blade needs it: see CmdSheathFit
//
// MESH .bin: int32 nprim; per primitive: int32 nv, int32 nt, float pos[nv][3], float nrm[nv][3], float tan[nv][4],
// uint32 idx[nt][3]. Positions are the glTF's (WolvenKit: x right, y up, -z forward; the game's (x, y, z) =
// glTF (x, -z, y)). WEAPON.bin: float32 xyz triples in the game's axes, the weapon's own frame (+y barrel, +z top).
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <map>
#include <numeric>
#include <set>
#include <string>
#include <unordered_map>
#include <vector>

namespace {

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
    return l > 1e-15 ? a * (1.0 / l) : V3{0, 0, 0};
}

struct Prim {
    std::vector<float> pos, nrm, tan;
    std::vector<uint32_t> idx;
    int nv() const { return int(pos.size() / 3); }
    int nt() const { return int(idx.size() / 3); }
    V3 P(int i) const { return {pos[3 * i], pos[3 * i + 1], pos[3 * i + 2]}; }
};
using Mesh = std::vector<Prim>;

Mesh LoadMesh(const char* path) {
    FILE* f = std::fopen(path, "rb");
    if (!f) {
        std::fprintf(stderr, "cannot open %s\n", path);
        std::exit(2);
    }
    int32_t np = 0;
    std::fread(&np, 4, 1, f);
    Mesh m(np);
    for (auto& p : m) {
        int32_t nv = 0, nt = 0;
        std::fread(&nv, 4, 1, f);
        std::fread(&nt, 4, 1, f);
        p.pos.resize(size_t(nv) * 3);
        p.nrm.resize(size_t(nv) * 3);
        p.tan.resize(size_t(nv) * 4);
        p.idx.resize(size_t(nt) * 3);
        std::fread(p.pos.data(), 4, p.pos.size(), f);
        std::fread(p.nrm.data(), 4, p.nrm.size(), f);
        std::fread(p.tan.data(), 4, p.tan.size(), f);
        std::fread(p.idx.data(), 4, p.idx.size(), f);
    }
    std::fclose(f);
    return m;
}

void SaveMesh(const char* path, const Mesh& m) {
    FILE* f = std::fopen(path, "wb");
    if (!f) {
        std::fprintf(stderr, "cannot write %s\n", path);
        std::exit(2);
    }
    const int32_t np = int32_t(m.size());
    std::fwrite(&np, 4, 1, f);
    for (const auto& p : m) {
        const int32_t nv = p.nv(), nt = p.nt();
        std::fwrite(&nv, 4, 1, f);
        std::fwrite(&nt, 4, 1, f);
        std::fwrite(p.pos.data(), 4, p.pos.size(), f);
        std::fwrite(p.nrm.data(), 4, p.nrm.size(), f);
        std::fwrite(p.tan.data(), 4, p.tan.size(), f);
        std::fwrite(p.idx.data(), 4, p.idx.size(), f);
    }
    std::fclose(f);
}

// glTF axes -> game axes
V3 Game(V3 g) { return {g.x, -g.z, g.y}; }

// ---------------------------------------------------------------- triangles and a grid over them

struct Tri {
    V3 a, b, c, n;
};

V3 ClosestOnTri(V3 p, V3 a, V3 b, V3 c) {
    const V3 ab = b - a, ac = c - a, ap = p - a;
    const double d1 = Dot(ab, ap), d2 = Dot(ac, ap);
    if (d1 <= 0 && d2 <= 0) return a;
    const V3 bp = p - b;
    const double d3 = Dot(ab, bp), d4 = Dot(ac, bp);
    if (d3 >= 0 && d4 <= d3) return b;
    const double vc = d1 * d4 - d3 * d2;
    if (vc <= 0 && d1 >= 0 && d3 <= 0) return a + ab * (d1 / (d1 - d3));
    const V3 cp = p - c;
    const double d5 = Dot(ab, cp), d6 = Dot(ac, cp);
    if (d6 >= 0 && d5 <= d6) return c;
    const double vb = d5 * d2 - d1 * d6;
    if (vb <= 0 && d2 >= 0 && d6 <= 0) return a + ac * (d2 / (d2 - d6));
    const double va = d3 * d6 - d5 * d4;
    if (va <= 0 && (d4 - d3) >= 0 && (d5 - d6) >= 0) return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    const double den = 1.0 / (va + vb + vc);
    return a + ab * (vb * den) + ac * (vc * den);
}

// Moller-Trumbore, both faces; t along d
bool RayTri(V3 o, V3 d, const Tri& t, double& hit) {
    const V3 e1 = t.b - t.a, e2 = t.c - t.a, pv = Cross(d, e2);
    const double det = Dot(e1, pv);
    if (std::fabs(det) < 1e-14) return false;
    const double inv = 1.0 / det;
    const V3 tv = o - t.a;
    const double u = Dot(tv, pv) * inv;
    if (u < 0 || u > 1) return false;
    const V3 qv = Cross(tv, e1);
    const double v = Dot(d, qv) * inv;
    if (v < 0 || u + v > 1) return false;
    hit = Dot(e2, qv) * inv;
    return true;
}

struct Grid {
    std::vector<Tri> tris;
    V3 lo, hi;
    double cell = 0.02;
    int n[3] = {1, 1, 1};
    std::vector<std::vector<int>> cells;
    int Index(int i, int j, int k) const { return (i * n[1] + j) * n[2] + k; }
    void Build(std::vector<Tri> t, double c) {
        tris = std::move(t);
        cell = c;
        lo = {1e9, 1e9, 1e9};
        hi = {-1e9, -1e9, -1e9};
        for (const Tri& q : tris)
            for (V3 v : {q.a, q.b, q.c}) {
                lo = {std::min(lo.x, v.x), std::min(lo.y, v.y), std::min(lo.z, v.z)};
                hi = {std::max(hi.x, v.x), std::max(hi.y, v.y), std::max(hi.z, v.z)};
            }
        lo = lo - V3{c, c, c};
        hi = hi + V3{c, c, c};
        n[0] = std::max(1, int((hi.x - lo.x) / c) + 1);
        n[1] = std::max(1, int((hi.y - lo.y) / c) + 1);
        n[2] = std::max(1, int((hi.z - lo.z) / c) + 1);
        cells.assign(size_t(n[0]) * n[1] * n[2], {});
        for (int t = 0; t < int(tris.size()); ++t) {
            const Tri& q = tris[t];
            V3 a{std::min({q.a.x, q.b.x, q.c.x}), std::min({q.a.y, q.b.y, q.c.y}), std::min({q.a.z, q.b.z, q.c.z})};
            V3 b{std::max({q.a.x, q.b.x, q.c.x}), std::max({q.a.y, q.b.y, q.c.y}), std::max({q.a.z, q.b.z, q.c.z})};
            const int i0 = Clamp(int((a.x - lo.x) / c), 0), i1 = Clamp(int((b.x - lo.x) / c), 0);
            const int j0 = Clamp(int((a.y - lo.y) / c), 1), j1 = Clamp(int((b.y - lo.y) / c), 1);
            const int k0 = Clamp(int((a.z - lo.z) / c), 2), k1 = Clamp(int((b.z - lo.z) / c), 2);
            for (int i = i0; i <= i1; ++i)
                for (int j = j0; j <= j1; ++j)
                    for (int k = k0; k <= k1; ++k) cells[Index(i, j, k)].push_back(t);
        }
    }
    int Clamp(int v, int axis) const { return std::max(0, std::min(n[axis] - 1, v)); }
    // the nearest point of the surface to p, searching shells of cells outward until it cannot get nearer
    bool Nearest(V3 p, V3& best, int& bestTri, double maxDist = 0.25) const {
        const int ci = Clamp(int((p.x - lo.x) / cell), 0), cj = Clamp(int((p.y - lo.y) / cell), 1),
                  ck = Clamp(int((p.z - lo.z) / cell), 2);
        double bd = 1e18;
        bestTri = -1;
        const int maxR = int(maxDist / cell) + 1;
        for (int r = 0; r <= maxR; ++r) {
            for (int i = ci - r; i <= ci + r; ++i)
                for (int j = cj - r; j <= cj + r; ++j)
                    for (int k = ck - r; k <= ck + r; ++k) {
                        if (std::max({std::abs(i - ci), std::abs(j - cj), std::abs(k - ck)}) != r) continue;
                        if (i < 0 || j < 0 || k < 0 || i >= n[0] || j >= n[1] || k >= n[2]) continue;
                        for (int t : cells[Index(i, j, k)]) {
                            const V3 q = ClosestOnTri(p, tris[t].a, tris[t].b, tris[t].c);
                            const double d = Dot(q - p, q - p);
                            if (d < bd) bd = d, best = q, bestTri = t;
                        }
                    }
            if (bestTri >= 0 && std::sqrt(bd) < r * cell) break;
        }
        return bestTri >= 0;
    }
    // the nearest crossing of the line o + s d (|s| <= maxS), either way along it
    bool Cross(V3 o, V3 d, double maxS, V3& hitP, int& hitTri) const {
        double bestS = 1e18;
        hitTri = -1;
        const int steps = int(2 * maxS / (cell * 0.5)) + 1;
        std::set<int> seen;
        for (int s = 0; s <= steps; ++s) {
            const V3 p = o + d * (-maxS + 2 * maxS * s / steps);
            const int i = Clamp(int((p.x - lo.x) / cell), 0), j = Clamp(int((p.y - lo.y) / cell), 1),
                      k = Clamp(int((p.z - lo.z) / cell), 2);
            for (int t : cells[Index(i, j, k)]) {
                if (!seen.insert(t).second) continue;
                double h = 0;
                if (RayTri(o, d, tris[t], h) && std::fabs(h) <= maxS && std::fabs(h) < std::fabs(bestS))
                    bestS = h, hitTri = t;
            }
        }
        if (hitTri < 0) return false;
        hitP = o + d * bestS;
        return true;
    }
};

std::vector<Tri> Triangles(const Mesh& m) {
    std::vector<Tri> out;
    for (const Prim& p : m)
        for (int t = 0; t < p.nt(); ++t) {
            Tri q{p.P(p.idx[3 * t]), p.P(p.idx[3 * t + 1]), p.P(p.idx[3 * t + 2]), {}};
            q.n = Unit(Cross(q.b - q.a, q.c - q.a));
            if (Len(q.n) > 0) out.push_back(q);
        }
    return out;
}

// ---------------------------------------------------------------- mirror

void MirrorMesh(Mesh& m) {
    for (Prim& p : m) {
        for (int i = 0; i < p.nv(); ++i) {
            p.pos[3 * i] = -p.pos[3 * i];
            p.nrm[3 * i] = -p.nrm[3 * i];
            p.tan[4 * i] = -p.tan[4 * i];
            p.tan[4 * i + 3] = -p.tan[4 * i + 3];
        }
        for (int t = 0; t < p.nt(); ++t) std::swap(p.idx[3 * t + 1], p.idx[3 * t + 2]);
    }
}

int CmdMirror(const char* in, const char* out) {
    Mesh m = LoadMesh(in);
    MirrorMesh(m);
    SaveMesh(out, m);
    std::printf("{\"mirrored\": %zu}\n", m.size());
    return 0;
}

// ---------------------------------------------------------------- welding by position (UV seams split vertices)

struct Welded {
    std::vector<std::vector<int>> id;   // per prim, per vertex: welded id
    int count = 0;
};
Welded Weld(const Mesh& m) {
    Welded w;
    std::map<std::array<int64_t, 3>, int> key;
    for (const Prim& p : m) {
        std::vector<int> ids(p.nv());
        for (int i = 0; i < p.nv(); ++i) {
            const std::array<int64_t, 3> k{std::llround(p.pos[3 * i] * 1e5), std::llround(p.pos[3 * i + 1] * 1e5),
                                           std::llround(p.pos[3 * i + 2] * 1e5)};
            auto it = key.find(k);
            if (it == key.end()) it = key.emplace(k, w.count++).first;
            ids[i] = it->second;
        }
        w.id.push_back(std::move(ids));
    }
    return w;
}

// ---------------------------------------------------------------- quaternions

struct Q {
    double x = 0, y = 0, z = 0, w = 1;
};
Q QMul(Q a, Q b) {
    return {a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y, a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
            a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w, a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z};
}
V3 QRot(Q q, V3 v) {
    const V3 u{q.x, q.y, q.z};
    const V3 t = Cross(u, v) * 2.0;
    return v + t * q.w + Cross(u, t);
}
Q QFromVec(V3 r) {   // rotation vector -> quaternion
    const double a = Len(r);
    if (a < 1e-12) return {0, 0, 0, 1};
    const V3 n = r * (1.0 / a);
    const double s = std::sin(0.5 * a);
    return {n.x * s, n.y * s, n.z * s, std::cos(0.5 * a)};
}
Q QFromBasis(V3 X, V3 Y, V3 Z) {   // the rotation taking the unit axes to X, Y, Z (columns)
    const double m00 = X.x, m01 = Y.x, m02 = Z.x, m10 = X.y, m11 = Y.y, m12 = Z.y, m20 = X.z, m21 = Y.z, m22 = Z.z;
    const double tr = m00 + m11 + m22;
    Q q;
    if (tr > 0) {
        const double s = std::sqrt(tr + 1.0) * 2;
        q = {(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s};
    } else if (m00 > m11 && m00 > m22) {
        const double s = std::sqrt(1.0 + m00 - m11 - m22) * 2;
        q = {0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s};
    } else if (m11 > m22) {
        const double s = std::sqrt(1.0 + m11 - m00 - m22) * 2;
        q = {(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s};
    } else {
        const double s = std::sqrt(1.0 + m22 - m00 - m11) * 2;
        q = {(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s};
    }
    const double l = std::sqrt(q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w);
    return {q.x / l, q.y / l, q.z / l, q.w / l};
}

// The rotation that best takes the centred points a onto b, from S = sum a b^T (Horn's quaternion: the top
// eigenvector of his 4x4 matrix, by power iteration shifted to make it the largest)
Q HornRotation(const double S[3][3]) {
    const double N[4][4] = {
        {S[0][0] + S[1][1] + S[2][2], S[1][2] - S[2][1], S[2][0] - S[0][2], S[0][1] - S[1][0]},
        {S[1][2] - S[2][1], S[0][0] - S[1][1] - S[2][2], S[0][1] + S[1][0], S[2][0] + S[0][2]},
        {S[2][0] - S[0][2], S[0][1] + S[1][0], -S[0][0] + S[1][1] - S[2][2], S[1][2] + S[2][1]},
        {S[0][1] - S[1][0], S[2][0] + S[0][2], S[1][2] + S[2][1], -S[0][0] - S[1][1] + S[2][2]}};
    double shiftI = 0;
    for (int r = 0; r < 4; ++r)
        for (int c = 0; c < 4; ++c) shiftI += std::fabs(N[r][c]);
    double v[4] = {1, 0, 0, 0};
    for (int it = 0; it < 500; ++it) {
        double nv[4];
        for (int r = 0; r < 4; ++r) {
            nv[r] = shiftI * v[r];
            for (int c = 0; c < 4; ++c) nv[r] += N[r][c] * v[c];
        }
        const double l = std::sqrt(nv[0] * nv[0] + nv[1] * nv[1] + nv[2] * nv[2] + nv[3] * nv[3]);
        for (int r = 0; r < 4; ++r) v[r] = nv[r] / l;
    }
    return {v[1], v[2], v[3], v[0]};
}

// ---------------------------------------------------------------- refit

// rigid: the piece is moved as a whole (a holster keeps its shape): the rotation and shift that best match the
// per-vertex moves (Horn's quaternion), instead of the moves themselves
int CmdRefit(const char* fromPath, const char* toPath, const char* in, const char* out, bool rigid = false) {
    Grid from, to;
    from.Build(Triangles(LoadMesh(fromPath)), 0.02);
    to.Build(Triangles(LoadMesh(toPath)), 0.02);
    Mesh m = LoadMesh(in);
    const Welded w = Weld(m);
    std::vector<V3> disp(w.count, V3{}), base(w.count, V3{});
    std::vector<int> got(w.count, 0);
    double maxD = 0, sumD = 0;
    int misses = 0;
    for (size_t pi = 0; pi < m.size(); ++pi) {
        const Prim& p = m[pi];
        for (int i = 0; i < p.nv(); ++i) {
            const int id = w.id[pi][i];
            if (got[id]) continue;
            got[id] = 1;
            const V3 v = p.P(i);
            base[id] = v;
            V3 q;
            int t = -1;
            if (!from.Nearest(v, q, t)) {
                ++misses;
                continue;
            }
            const V3 n = from.tris[t].n;
            const V3 off = v - q;
            const double along = Dot(off, n);
            const V3 side = off - n * along;
            V3 q2;
            int t2 = -1;
            if (!to.Cross(q, n, 0.06, q2, t2)) {
                if (!to.Nearest(q, q2, t2)) {
                    ++misses;
                    continue;
                }
            }
            V3 n2 = to.tris[t2].n;
            if (Dot(n2, n) < 0) n2 = n2 * -1.0;
            const V3 v2 = q2 + n2 * along + side;
            disp[id] = v2 - v;
            maxD = std::max(maxD, Len(disp[id]));
            sumD += Len(disp[id]);
        }
    }
    // smoothing over the garment's own edges: a jump of the nearest body point (arm to chest, thigh to thigh) shows
    // as one vertex moved far from its neighbours
    std::vector<std::set<int>> nb(w.count);
    for (size_t pi = 0; pi < m.size(); ++pi) {
        const Prim& p = m[pi];
        for (int t = 0; t < p.nt(); ++t) {
            const int a = w.id[pi][p.idx[3 * t]], b = w.id[pi][p.idx[3 * t + 1]], c = w.id[pi][p.idx[3 * t + 2]];
            nb[a].insert(b), nb[a].insert(c), nb[b].insert(a), nb[b].insert(c), nb[c].insert(a), nb[c].insert(b);
        }
    }
    for (int it = 0; it < 12; ++it) {
        std::vector<V3> next = disp;
        for (int i = 0; i < w.count; ++i) {
            if (nb[i].empty()) continue;
            V3 s{};
            for (int j : nb[i]) s = s + disp[j];
            next[i] = disp[i] * 0.5 + s * (0.5 / double(nb[i].size()));
        }
        disp = next;
    }
    if (rigid) {
        V3 ac{}, bc{};
        int n = 0;
        for (int i = 0; i < w.count; ++i)
            if (got[i]) ac = ac + base[i], bc = bc + base[i] + disp[i], ++n;
        ac = ac * (1.0 / n), bc = bc * (1.0 / n);
        double S[3][3] = {};
        for (int i = 0; i < w.count; ++i) {
            if (!got[i]) continue;
            const V3 a = base[i] - ac, b = base[i] + disp[i] - bc;
            const double av[3] = {a.x, a.y, a.z}, bv[3] = {b.x, b.y, b.z};
            for (int r = 0; r < 3; ++r)
                for (int c = 0; c < 3; ++c) S[r][c] += av[r] * bv[c];
        }
        const Q q = HornRotation(S);
        const V3 t = bc - QRot(q, ac);
        double rmsErr = 0;
        for (int i = 0; i < w.count; ++i)
            if (got[i]) {
                const V3 e = QRot(q, base[i]) + t - (base[i] + disp[i]);
                rmsErr += Dot(e, e);
            }
        for (Prim& p : m)
            for (int i = 0; i < p.nv(); ++i) {
                const V3 P = QRot(q, p.P(i)) + t;
                const V3 nn = QRot(q, {p.nrm[3 * i], p.nrm[3 * i + 1], p.nrm[3 * i + 2]});
                const V3 tg = QRot(q, {p.tan[4 * i], p.tan[4 * i + 1], p.tan[4 * i + 2]});
                p.pos[3 * i] = float(P.x), p.pos[3 * i + 1] = float(P.y), p.pos[3 * i + 2] = float(P.z);
                p.nrm[3 * i] = float(nn.x), p.nrm[3 * i + 1] = float(nn.y), p.nrm[3 * i + 2] = float(nn.z);
                p.tan[4 * i] = float(tg.x), p.tan[4 * i + 1] = float(tg.y), p.tan[4 * i + 2] = float(tg.z);
            }
        SaveMesh(out, m);
        std::printf("{\"vertices\": %d, \"misses\": %d, \"rigid\": true, \"turnDeg\": %.2f, \"shiftMm\": %.1f, "
                    "\"rmsLeftMm\": %.1f}\n", w.count, misses, 2 * std::acos(std::min(1.0, std::fabs(q.w))) * 57.29578,
                    1e3 * Len(bc - ac), 1e3 * std::sqrt(rmsErr / n));
        return 0;
    }
    for (size_t pi = 0; pi < m.size(); ++pi) {
        Prim& p = m[pi];
        for (int i = 0; i < p.nv(); ++i) {
            const V3 d = disp[w.id[pi][i]];
            p.pos[3 * i] += float(d.x);
            p.pos[3 * i + 1] += float(d.y);
            p.pos[3 * i + 2] += float(d.z);
        }
    }
    SaveMesh(out, m);
    std::printf("{\"vertices\": %d, \"misses\": %d, \"meanMoveMm\": %.2f, \"maxMoveMm\": %.2f}\n", w.count, misses,
                1e3 * sumD / std::max(1, w.count - misses), 1e3 * maxD);
    return 0;
}

// ---------------------------------------------------------------- connected pieces

struct Piece {
    int prim, id;
    std::vector<int> verts, tris;
};

std::vector<Piece> Pieces(const Mesh& m) {
    const Welded w = Weld(m);
    std::vector<int> parent(w.count);
    std::iota(parent.begin(), parent.end(), 0);
    std::function<int(int)> find = [&](int x) { return parent[x] == x ? x : parent[x] = find(parent[x]); };
    for (size_t pi = 0; pi < m.size(); ++pi) {
        const Prim& p = m[pi];
        for (int t = 0; t < p.nt(); ++t) {
            const int a = find(w.id[pi][p.idx[3 * t]]), b = find(w.id[pi][p.idx[3 * t + 1]]),
                      c = find(w.id[pi][p.idx[3 * t + 2]]);
            parent[b] = a;
            parent[find(c)] = a;
        }
    }
    std::vector<Piece> out;
    for (size_t pi = 0; pi < m.size(); ++pi) {
        const Prim& p = m[pi];
        std::map<int, int> slot;
        std::vector<Piece> local;
        for (int t = 0; t < p.nt(); ++t) {
            const int root = find(w.id[pi][p.idx[3 * t]]);
            auto it = slot.find(root);
            if (it == slot.end()) {
                it = slot.emplace(root, int(local.size())).first;
                local.push_back({int(pi), int(local.size()), {}, {}});
            }
            local[it->second].tris.push_back(t);
        }
        for (Piece& pc : local) {
            std::set<int> vs;
            for (int t : pc.tris)
                for (int k = 0; k < 3; ++k) vs.insert(p.idx[3 * t + k]);
            pc.verts.assign(vs.begin(), vs.end());
            out.push_back(std::move(pc));
        }
    }
    return out;
}

int CmdParts(const char* in) {
    const Mesh m = LoadMesh(in);
    for (const Piece& pc : Pieces(m)) {
        V3 lo{1e9, 1e9, 1e9}, hi{-1e9, -1e9, -1e9}, c{};
        for (int v : pc.verts) {
            const V3 g = Game(m[pc.prim].P(v));
            lo = {std::min(lo.x, g.x), std::min(lo.y, g.y), std::min(lo.z, g.z)};
            hi = {std::max(hi.x, g.x), std::max(hi.y, g.y), std::max(hi.z, g.z)};
            c = c + g;
        }
        c = c * (1.0 / std::max<size_t>(1, pc.verts.size()));
        std::printf("{\"prim\": %d, \"piece\": %d, \"verts\": %zu, \"tris\": %zu, \"centre\": [%.4f, %.4f, %.4f], "
                    "\"size\": [%.4f, %.4f, %.4f]}\n",
                    pc.prim, pc.id, pc.verts.size(), pc.tris.size(), c.x, c.y, c.z, hi.x - lo.x, hi.y - lo.y,
                    hi.z - lo.z);
    }
    return 0;
}

std::set<std::pair<int, int>> ParseList(const char* s) {
    std::set<std::pair<int, int>> out;
    std::string str(s);
    size_t pos = 0;
    while (pos < str.size()) {
        size_t comma = str.find(',', pos);
        if (comma == std::string::npos) comma = str.size();
        const std::string item = str.substr(pos, comma - pos);
        const size_t colon = item.find(':');
        if (colon != std::string::npos) out.insert({std::atoi(item.substr(0, colon).c_str()), std::atoi(item.c_str() + colon + 1)});
        pos = comma + 1;
    }
    return out;
}

// drop: the listed pieces' triangles go; keep: every triangle but the listed pieces' goes. Vertices stay (the glb
// written back keeps its vertex arrays).
int CmdDrop(const char* in, const char* out, const char* list, bool keepListed = false) {
    Mesh m = LoadMesh(in);
    const auto drop = ParseList(list);
    std::vector<std::set<int>> dropTris(m.size());
    for (const Piece& pc : Pieces(m))
        if ((drop.count({pc.prim, pc.id}) != 0) != keepListed)
            for (int t : pc.tris) dropTris[pc.prim].insert(t);
    int removed = 0;
    for (size_t pi = 0; pi < m.size(); ++pi) {
        Prim& p = m[pi];
        std::vector<uint32_t> keep;
        for (int t = 0; t < p.nt(); ++t) {
            if (dropTris[pi].count(t)) {
                ++removed;
                continue;
            }
            keep.insert(keep.end(), {p.idx[3 * t], p.idx[3 * t + 1], p.idx[3 * t + 2]});
        }
        p.idx = std::move(keep);
    }
    SaveMesh(out, m);
    std::printf("{\"removedTriangles\": %d}\n", removed);
    return 0;
}

int CmdPieces(const char* in, const char* out, const char* list) {
    const Mesh m = LoadMesh(in);
    const auto want = ParseList(list);
    FILE* f = std::fopen(out, "wb");
    int n = 0;
    for (const Piece& pc : Pieces(m))
        if (want.count({pc.prim, pc.id}))
            for (int v : pc.verts) {
                const V3 g = Game(m[pc.prim].P(v));
                const float x[3] = {float(g.x), float(g.y), float(g.z)};
                std::fwrite(x, 4, 3, f);
                ++n;
            }
    std::fclose(f);
    std::printf("{\"points\": %d}\n", n);
    return 0;
}

// ---------------------------------------------------------------- fit: a weapon in a holster

// principal axes, largest spread first
void Pca(const std::vector<V3>& pts, V3& centre, V3 ax[3]) {
    centre = {};
    for (const V3& p : pts) centre = centre + p;
    centre = centre * (1.0 / pts.size());
    double c[3][3] = {};
    for (const V3& p : pts) {
        const double d[3] = {p.x - centre.x, p.y - centre.y, p.z - centre.z};
        for (int i = 0; i < 3; ++i)
            for (int j = 0; j < 3; ++j) c[i][j] += d[i] * d[j];
    }
    // power iteration with deflation
    V3 found[3];
    double mat[3][3];
    std::memcpy(mat, c, sizeof(mat));
    for (int k = 0; k < 3; ++k) {
        V3 v{1, 0.3, 0.1};
        if (k == 1) v = {0.2, 1, 0.3};
        if (k == 2) v = {0.1, 0.2, 1};
        for (int it = 0; it < 200; ++it) {
            V3 nv{mat[0][0] * v.x + mat[0][1] * v.y + mat[0][2] * v.z, mat[1][0] * v.x + mat[1][1] * v.y + mat[1][2] * v.z,
                  mat[2][0] * v.x + mat[2][1] * v.y + mat[2][2] * v.z};
            for (int j = 0; j < k; ++j) nv = nv - found[j] * Dot(nv, found[j]);
            v = Unit(nv);
        }
        found[k] = v;
        const double lam = Dot(v, V3{mat[0][0] * v.x + mat[0][1] * v.y + mat[0][2] * v.z,
                                     mat[1][0] * v.x + mat[1][1] * v.y + mat[1][2] * v.z,
                                     mat[2][0] * v.x + mat[2][1] * v.y + mat[2][2] * v.z});
        const double vv[3] = {v.x, v.y, v.z};
        for (int i = 0; i < 3; ++i)
            for (int j = 0; j < 3; ++j) mat[i][j] -= lam * vv[i] * vv[j];
    }
    for (int k = 0; k < 3; ++k) ax[k] = found[k];
    if (Dot(Cross(ax[0], ax[1]), ax[2]) < 0) ax[2] = ax[2] * -1.0;
}

// ---------------------------------------------------------------- place: a rigid piece put onto the body

V3 Gltf(V3 g) { return {g.x, g.z, -g.y}; }   // the game's axes -> glTF

// A straight sheath hung from a harness strap: its throat (the top of FRAME's long axis) at `anchor`, its long axis
// along `blade` (the old knife's +z, laid onto the body's surface there), its flat back toward the body and `gap` off
// the skin at the nearest point. The same rigid move goes to every IN -> OUT pair. Game axes in, glTF in the files.
//   place BODY.bin FRAME.bin anchor=x,y,z blade=qx,qy,qz,qw gap=G IN OUT [IN OUT ...]
int CmdPlace(int argc, char** argv) {
    Grid body;
    body.Build(Triangles(LoadMesh(argv[2])), 0.02);
    const Mesh frame = LoadMesh(argv[3]);
    V3 anchor{};
    Q blade;
    double gap = 0.005;
    int first = 4;
    for (; first < argc; ++first) {
        double a, b, c, d;
        if (std::sscanf(argv[first], "anchor=%lf,%lf,%lf", &a, &b, &c) == 3) anchor = {a, b, c};
        else if (std::sscanf(argv[first], "blade=%lf,%lf,%lf,%lf", &a, &b, &c, &d) == 4) blade = {a, b, c, d};
        else if (std::sscanf(argv[first], "gap=%lf", &a) == 1) gap = a;
        else break;
    }
    // the frame piece's own axes (game axes), from the vertices its triangles use
    std::vector<V3> pts;
    for (const Prim& p : frame) {
        std::vector<char> used(p.nv(), 0);
        for (uint32_t i : p.idx) used[i] = 1;
        for (int i = 0; i < p.nv(); ++i)
            if (used[i]) pts.push_back(Game(p.P(i)));
    }
    V3 c, ax[3];
    Pca(pts, c, ax);
    V3 u = Dot(ax[0], V3{0, 0, -1}) >= 0 ? ax[0] : ax[0] * -1.0;     // down the sheath, toward its tip
    V3 t = ax[2];                                                     // through it: away from the body
    {
        V3 q;
        int tri;
        body.Nearest(Gltf(c), q, tri, 0.5);
        if (Dot(t, Unit(c - Game(q))) < 0) t = t * -1.0;
    }
    const V3 w = Cross(t, u);
    double sMin = 1e9;
    for (const V3& p : pts) sMin = std::min(sMin, Dot(p - c, u));
    const V3 throat = c + u * sMin;

    // the target frame at the anchor
    V3 B;
    int bt;
    body.Nearest(Gltf(anchor), B, bt, 0.5);
    V3 N = Unit(anchor - Game(B));
    const V3 D = QRot(blade, {0, 0, 1});
    const V3 U = Unit(D - N * Dot(D, N));
    const V3 W = Cross(N, U);
    // R takes u -> U, t -> N, w -> W
    auto rot = [&](V3 v) { return U * Dot(v, u) + N * Dot(v, t) + W * Dot(v, w); };
    V3 T = anchor - rot(throat);
    // the back onto the body: the nearest approach of the frame to the skin made `gap` (twice: the nearest points
    // move with it)
    const V3 outward0 = Unit(Game(body.tris[bt].n));
    const double windingOut = Dot(outward0, N) >= 0 ? 1.0 : -1.0;
    double shift = 0, minGap = 0;
    for (int it = 0; it < 3; ++it) {
        minGap = 1e9;
        for (const V3& p : pts) {
            const V3 q = rot(p) + T;
            V3 nb;
            int tri;
            if (!body.Nearest(Gltf(q), nb, tri, 0.3)) continue;
            const V3 n = Unit(Game(body.tris[tri].n)) * windingOut;
            minGap = std::min(minGap, Dot(q - Game(nb), n));
        }
        const double s = gap - minGap;
        T = T + N * s;
        shift += s;
        if (std::fabs(s) < 1e-4) break;
    }
    for (int i = first; i + 1 < argc; i += 2) {
        Mesh m = LoadMesh(argv[i]);
        for (Prim& p : m)
            for (int v = 0; v < p.nv(); ++v) {
                const V3 P = Gltf(rot(Game(p.P(v))) + T);
                const V3 n = Gltf(rot(Game({p.nrm[3 * v], p.nrm[3 * v + 1], p.nrm[3 * v + 2]})));
                const V3 g = Gltf(rot(Game({p.tan[4 * v], p.tan[4 * v + 1], p.tan[4 * v + 2]})));
                p.pos[3 * v] = float(P.x), p.pos[3 * v + 1] = float(P.y), p.pos[3 * v + 2] = float(P.z);
                p.nrm[3 * v] = float(n.x), p.nrm[3 * v + 1] = float(n.y), p.nrm[3 * v + 2] = float(n.z);
                p.tan[4 * v] = float(g.x), p.tan[4 * v + 1] = float(g.y), p.tan[4 * v + 2] = float(g.z);
            }
        SaveMesh(argv[i + 1], m);
    }
    std::printf("{\"throat\": [%.4f, %.4f, %.4f], \"normal\": [%.3f, %.3f, %.3f], \"along\": [%.3f, %.3f, %.3f], "
                "\"pushMm\": %.1f, \"gapMm\": %.1f, \"lengthMm\": %.1f}\n",
                anchor.x, anchor.y, anchor.z, N.x, N.y, N.z, U.x, U.y, U.z, 1e3 * shift, 1e3 * (minGap + 0.0),
                1e3 * ([&] { double hi = -1e9; for (const V3& p : pts) hi = std::max(hi, Dot(p - c, u)); return hi; }() - sMin));
    return 0;
}

// THE HOLSTER AS TWO WALLS. A gun holster is flat: seen along its thinnest axis, every point of the pocket has the
// surface on both sides. The holster's own frame (u along its length toward the closed end, w across, t through it)
// is rasterised at 1 mm in (u, w); each cell keeps every crossing of the surface along t, sorted. A point is in the
// pocket when its cell has crossings round it: between the 2nd and the next-to-last of four or more (outer wall,
// inner wall ... inner wall, outer wall), between the two of two (a single sheet).
struct Pocket {
    V3 centre, u, w, t;
    double res = 0.001;
    int nu = 0, nw = 0;
    double u0 = 0, w0 = 0;
    std::vector<std::vector<float>> hits;
    double uMax = 0;   // the closed end
    void Build(const std::vector<Tri>& tris) {
        double ulo = 1e9, uhi = -1e9, wlo = 1e9, whi = -1e9;
        for (const Tri& q : tris)
            for (V3 v : {q.a, q.b, q.c}) {
                const V3 d = v - centre;
                ulo = std::min(ulo, Dot(d, u)), uhi = std::max(uhi, Dot(d, u));
                wlo = std::min(wlo, Dot(d, w)), whi = std::max(whi, Dot(d, w));
            }
        u0 = ulo, w0 = wlo, uMax = uhi;
        nu = int((uhi - ulo) / res) + 2, nw = int((whi - wlo) / res) + 2;
        hits.assign(size_t(nu) * nw, {});
        for (const Tri& q : tris) {
            const V3 P[3] = {q.a, q.b, q.c};
            double U[3], W[3], T[3];
            for (int k = 0; k < 3; ++k) {
                const V3 d = P[k] - centre;
                U[k] = Dot(d, u), W[k] = Dot(d, w), T[k] = Dot(d, t);
            }
            const int i0 = std::max(0, int((std::min({U[0], U[1], U[2]}) - u0) / res));
            const int i1 = std::min(nu - 1, int((std::max({U[0], U[1], U[2]}) - u0) / res) + 1);
            const int j0 = std::max(0, int((std::min({W[0], W[1], W[2]}) - w0) / res));
            const int j1 = std::min(nw - 1, int((std::max({W[0], W[1], W[2]}) - w0) / res) + 1);
            const double den = (W[1] - W[2]) * (U[0] - U[2]) + (U[2] - U[1]) * (W[0] - W[2]);
            if (std::fabs(den) < 1e-14) continue;
            for (int i = i0; i <= i1; ++i)
                for (int j = j0; j <= j1; ++j) {
                    const double cu = u0 + (i + 0.5) * res, cw = w0 + (j + 0.5) * res;
                    const double a = ((W[1] - W[2]) * (cu - U[2]) + (U[2] - U[1]) * (cw - W[2])) / den;
                    const double b = ((W[2] - W[0]) * (cu - U[2]) + (U[0] - U[2]) * (cw - W[2])) / den;
                    const double c = 1 - a - b;
                    if (a < 0 || b < 0 || c < 0) continue;
                    hits[size_t(i) * nw + j].push_back(float(a * T[0] + b * T[1] + c * T[2]));
                }
        }
        for (auto& h : hits) std::sort(h.begin(), h.end());
    }
    // how far a point is from being in the pocket (0 inside with `clear` to spare); `wall` true when it is inside
    // the material of a wall instead
    double Outside(V3 p, double clear, bool* wall = nullptr) const {
        const V3 d = p - centre;
        const double U = Dot(d, u), W = Dot(d, w), T = Dot(d, t);
        const int i = int((U - u0) / res), j = int((W - w0) / res);
        if (wall) *wall = false;
        if (i < 0 || j < 0 || i >= nu || j >= nw) return 0.01;
        const auto& h = hits[size_t(i) * nw + j];
        if (h.size() < 2) return 0.006;
        double lo, hi;
        if (h.size() >= 4) lo = h[1], hi = h[h.size() - 2];
        else lo = h.front(), hi = h.back();
        if (wall) *wall = (T > h.front() && T < lo) || (T < h.back() && T > hi);
        if (T < lo + clear) return lo + clear - T;
        if (T > hi - clear) return T - (hi - clear);
        return 0;
    }
};

double Nelder(const std::function<double(const double*)>& f, double* x, int n, double step, int iters) {
    std::vector<std::vector<double>> s(n + 1, std::vector<double>(x, x + n));
    std::vector<double> v(n + 1);
    for (int i = 1; i <= n; ++i) s[i][i - 1] += step;
    for (int i = 0; i <= n; ++i) v[i] = f(s[i].data());
    std::vector<double> c(n), r(n), e(n);
    for (int it = 0; it < iters; ++it) {
        std::vector<int> o(n + 1);
        std::iota(o.begin(), o.end(), 0);
        std::sort(o.begin(), o.end(), [&](int a, int b) { return v[a] < v[b]; });
        const int hi = o[n], lo = o[0];
        std::fill(c.begin(), c.end(), 0.0);
        for (int i = 0; i <= n; ++i)
            if (i != hi)
                for (int k = 0; k < n; ++k) c[k] += s[i][k] / n;
        auto at = [&](double a, std::vector<double>& out) {
            for (int k = 0; k < n; ++k) out[k] = c[k] + a * (s[hi][k] - c[k]);
            return f(out.data());
        };
        const double vr = at(-1.0, r);
        if (vr < v[lo]) {
            const double ve = at(-2.0, e);
            if (ve < vr) s[hi] = e, v[hi] = ve;
            else s[hi] = r, v[hi] = vr;
        } else if (vr < v[o[n - 1]]) {
            s[hi] = r, v[hi] = vr;
        } else {
            const double vc = at(0.5, e);
            if (vc < v[hi]) s[hi] = e, v[hi] = vc;
            else
                for (int i = 0; i <= n; ++i)
                    if (i != lo) {
                        for (int k = 0; k < n; ++k) s[i][k] = s[lo][k] + 0.5 * (s[i][k] - s[lo][k]);
                        v[i] = f(s[i].data());
                    }
        }
    }
    int best = 0;
    for (int i = 1; i <= n; ++i)
        if (v[i] < v[best]) best = i;
    std::copy(s[best].begin(), s[best].end(), x);
    return v[best];
}

std::vector<V3> LoadPoints(const char* path) {
    std::vector<V3> out;
    FILE* f = std::fopen(path, "rb");
    if (!f) {
        std::fprintf(stderr, "cannot open %s\n", path);
        std::exit(2);
    }
    float p[3];
    while (std::fread(p, 4, 3, f) == 3) out.push_back({p[0], p[1], p[2]});
    std::fclose(f);
    return out;
}

// options: down=x,y,z  the holster's closed end points this way in the game's axes (default 0,0,-1)
//          axis=y|z    the weapon's insertion axis (+y guns, +z blades); cut=F share of its length that goes in
//          side=x,y,z  which way the weapon's +x should face roughly (optional: chooses between the two starts)
//          start=x,y,z an approximate place for the weapon's inserted part (optional)
int CmdFit(int argc, char** argv) {
    const Mesh hm = LoadMesh(argv[2]);
    std::vector<Tri> tris;
    std::vector<V3> hp;
    for (const Prim& p : hm) {
        for (int t = 0; t < p.nt(); ++t) {
            Tri q{Game(p.P(p.idx[3 * t])), Game(p.P(p.idx[3 * t + 1])), Game(p.P(p.idx[3 * t + 2])), {}};
            q.n = Unit(Cross(q.b - q.a, q.c - q.a));
            tris.push_back(q);
            hp.push_back((q.a + q.b + q.c) * (1.0 / 3.0));
        }
    }
    std::vector<V3> wp = LoadPoints(argv[3]);
    V3 down{0, 0, -1}, sideHint{}, start{};
    bool haveSide = false, haveStart = false;
    char axis = 'y';
    double cut = 0.6, clear = 0.0008;
    for (int i = 4; i < argc; ++i) {
        double a, b, c;
        if (std::sscanf(argv[i], "down=%lf,%lf,%lf", &a, &b, &c) == 3) down = Unit({a, b, c});
        else if (std::sscanf(argv[i], "side=%lf,%lf,%lf", &a, &b, &c) == 3) sideHint = Unit({a, b, c}), haveSide = true;
        else if (std::sscanf(argv[i], "start=%lf,%lf,%lf", &a, &b, &c) == 3) start = {a, b, c}, haveStart = true;
        else if (std::strncmp(argv[i], "axis=", 5) == 0) axis = argv[i][5];
        else if (std::sscanf(argv[i], "cut=%lf", &a) == 1) cut = a;
        else if (std::sscanf(argv[i], "clear=%lf", &a) == 1) clear = a;
    }
    // the weapon: its insertion axis and the part that goes in (from the tip back `cut` of its length)
    auto along = [&](V3 p) { return axis == 'z' ? p.z : p.y; };
    double lo = 1e9, hi = -1e9;
    for (const V3& p : wp) lo = std::min(lo, along(p)), hi = std::max(hi, along(p));
    std::vector<V3> inPart, outPart;
    for (size_t i = 0; i < wp.size(); i += std::max<size_t>(1, wp.size() / 4000)) {
        if (along(wp[i]) > hi - cut * (hi - lo)) inPart.push_back(wp[i]);
        else outPart.push_back(wp[i]);
    }
    V3 tipLocal{};
    {
        int n = 0;
        for (const V3& p : wp)
            if (along(p) > hi - 0.03 * (hi - lo)) tipLocal = tipLocal + p, ++n;
        tipLocal = tipLocal * (1.0 / std::max(1, n));
    }
    V3 inCentre{};
    for (const V3& p : inPart) inCentre = inCentre + p;
    inCentre = inCentre * (1.0 / inPart.size());

    // the holster's frame
    Pocket pk;
    V3 ax[3];
    Pca(hp, pk.centre, ax);
    pk.u = Dot(ax[0], down) >= 0 ? ax[0] : ax[0] * -1.0;
    pk.t = ax[2];
    pk.w = Unit(Cross(pk.t, pk.u));
    pk.Build(tris);
    const V3 closed = pk.centre + pk.u * pk.uMax;

    // starts: the weapon's insertion axis along u (tip at the closed end), its x either way along t
    struct Start {
        Q q;
        V3 p;
    };
    std::vector<Start> starts;
    for (double sgn : {1.0, -1.0}) {
        const V3 A = pk.u;              // weapon insertion axis -> u
        const V3 X = pk.t * sgn;        // weapon +x -> +-t
        V3 Xw, Yw, Zw;
        if (axis == 'z') {
            Zw = A, Xw = X, Yw = Cross(Zw, Xw);
        } else {
            Yw = A, Xw = X, Zw = Cross(Xw, Yw);
        }
        const Q q = QFromBasis(Xw, Yw, Zw);
        if (haveSide && Dot(QRot(q, {1, 0, 0}), sideHint) < 0) continue;
        V3 target = haveStart ? start : (pk.centre + pk.u * (0.25 * pk.uMax));
        starts.push_back({q, target - QRot(q, inCentre)});
    }
    double bestE = 1e30;
    Q bestQ;
    V3 bestP;
    double bestIn = 0, bestWall = 0;
    for (const Start& s0 : starts) {
        for (double du : {-0.02, 0.0, 0.02}) {
            auto pose = [&](const double* x, Q& q, V3& p) {
                q = QMul(QFromVec({x[3], x[4], x[5]}), s0.q);
                p = s0.p + pk.u * du + V3{x[0], x[1], x[2]};
            };
            auto cost = [&](const double* x) {
                Q q;
                V3 p;
                pose(x, q, p);
                double e = 0;
                for (const V3& w : inPart) {
                    const double o = pk.Outside(p + QRot(q, w), clear);
                    e += o * o;
                }
                e /= inPart.size();
                double walls = 0;
                for (const V3& w : outPart) {
                    bool wall = false;
                    pk.Outside(p + QRot(q, w), clear, &wall);
                    if (wall) walls += 1;
                }
                e += 4e-5 * walls / std::max<size_t>(1, outPart.size());
                // as deep as it goes: the tip toward the closed end
                const V3 tip = p + QRot(q, tipLocal);
                const double gap = Dot(closed - tip, pk.u);
                e += 2e-4 * gap * gap;
                // and no further turn than it needs
                e += 1e-6 * (x[3] * x[3] + x[4] * x[4] + x[5] * x[5]);
                return e;
            };
            double x[6] = {0, 0, 0, 0, 0, 0};
            double e = Nelder(cost, x, 6, 0.01, 900);
            e = Nelder(cost, x, 6, 0.003, 900);
            e = Nelder(cost, x, 6, 0.001, 600);
            if (e < bestE) {
                bestE = e;
                pose(x, bestQ, bestP);
                int in = 0, wall = 0;
                for (const V3& w : inPart) in += pk.Outside(bestP + QRot(bestQ, w), clear) == 0 ? 1 : 0;
                for (const V3& w : outPart) {
                    bool wl = false;
                    pk.Outside(bestP + QRot(bestQ, w), clear, &wl);
                    wall += wl ? 1 : 0;
                }
                bestIn = double(in) / inPart.size();
                bestWall = double(wall) / std::max<size_t>(1, outPart.size());
            }
        }
    }
    const V3 tip = bestP + QRot(bestQ, tipLocal);
    std::printf("{\"p\": [%.6f, %.6f, %.6f], \"q\": [%.6f, %.6f, %.6f, %.6f], \"cost\": %.3g, \"inPocket\": %.3f, "
                "\"outInWalls\": %.3f, \"tipToEndMm\": %.1f, \"holsterLengthMm\": %.1f}\n",
                bestP.x, bestP.y, bestP.z, bestQ.x, bestQ.y, bestQ.z, bestQ.w, bestE, bestIn, bestWall,
                1e3 * Dot(closed - tip, pk.u), 1e3 * (pk.uMax - pk.u0));
    return 0;
}

// ---------------------------------------------------------------- fit2: a weapon in a holster, by enclosure

// THE HOLSTER AS A CAVITY. A holster is open at the mouth (and often at the muzzle end, or along a side for the
// trigger guard), so "between its walls" is not well defined; what is: a point in its cavity sees the holster in
// almost every direction. On a 3 mm voxel grid round the holster, each voxel keeps (1) its distance to the surface and
// (2) its enclosure, the share of 42 directions in which a ray from it meets the holster within 12 cm. The weapon's
// inserted part should be enclosed (>= ENCLOSED) and every point of the weapon clear of the walls.
struct Field {
    V3 lo;
    double h = 0.003;
    int n[3] = {1, 1, 1};
    std::vector<float> dist, encl;
    size_t I(int i, int j, int k) const { return (size_t(i) * n[1] + j) * n[2] + k; }
    // trilinear; outside the box: far from the surface, not enclosed
    void At(V3 p, double& d, double& e) const {
        const double fx = (p.x - lo.x) / h - 0.5, fy = (p.y - lo.y) / h - 0.5, fz = (p.z - lo.z) / h - 0.5;
        const int i = int(std::floor(fx)), j = int(std::floor(fy)), k = int(std::floor(fz));
        if (i < 0 || j < 0 || k < 0 || i + 1 >= n[0] || j + 1 >= n[1] || k + 1 >= n[2]) {
            d = 0.05, e = 0;
            return;
        }
        const double ax = fx - i, ay = fy - j, az = fz - k;
        d = e = 0;
        for (int c = 0; c < 8; ++c) {
            const int di = c & 1, dj = (c >> 1) & 1, dk = (c >> 2) & 1;
            const double wgt = (di ? ax : 1 - ax) * (dj ? ay : 1 - ay) * (dk ? az : 1 - az);
            d += wgt * dist[I(i + di, j + dj, k + dk)];
            e += wgt * encl[I(i + di, j + dj, k + dk)];
        }
    }
};

// does the ray p + s dir (0 < s <= maxS) meet a triangle
bool RayHits(const Grid& g, V3 p, V3 dir, double maxS) {
    const double step = g.cell * 0.5;
    int lastCell = -1;
    for (double s = 0; s <= maxS + step; s += step) {
        const V3 q = p + dir * s;
        if (q.x < g.lo.x || q.y < g.lo.y || q.z < g.lo.z || q.x > g.hi.x || q.y > g.hi.y || q.z > g.hi.z) return false;
        const int i = g.Clamp(int((q.x - g.lo.x) / g.cell), 0), j = g.Clamp(int((q.y - g.lo.y) / g.cell), 1),
                  k = g.Clamp(int((q.z - g.lo.z) / g.cell), 2);
        const int cell = g.Index(i, j, k);
        if (cell == lastCell) continue;
        lastCell = cell;
        for (int t : g.cells[cell]) {
            double hit = 0;
            if (RayTri(p, dir, g.tris[t], hit) && hit > 1e-6 && hit <= maxS) return true;
        }
    }
    return false;
}

Field BuildField(const Grid& g, V3 lo, V3 hi) {
    Field f;
    f.lo = lo;
    for (int a = 0; a < 3; ++a) {
        const double span = a == 0 ? hi.x - lo.x : a == 1 ? hi.y - lo.y : hi.z - lo.z;
        f.n[a] = std::max(2, int(span / f.h) + 1);
    }
    const size_t total = size_t(f.n[0]) * f.n[1] * f.n[2];
    f.dist.assign(total, 0.05f);
    f.encl.assign(total, 0.0f);
    std::vector<V3> dirs;
    const int ND = 42;
    for (int i = 0; i < ND; ++i) {           // Fibonacci sphere
        const double z = 1 - 2 * (i + 0.5) / ND, r = std::sqrt(std::max(0.0, 1 - z * z));
        const double ph = i * 2.399963229728653;
        dirs.push_back({r * std::cos(ph), r * std::sin(ph), z});
    }
    for (int i = 0; i < f.n[0]; ++i)
        for (int j = 0; j < f.n[1]; ++j)
            for (int k = 0; k < f.n[2]; ++k) {
                const V3 p{lo.x + (i + 0.5) * f.h, lo.y + (j + 0.5) * f.h, lo.z + (k + 0.5) * f.h};
                V3 q;
                int t;
                const double d = g.Nearest(p, q, t, 0.05) ? Len(q - p) : 0.05;
                f.dist[f.I(i, j, k)] = float(d);
                if (d < 0.001) continue;
                int hits = 0;
                for (const V3& dir : dirs) hits += RayHits(g, p, dir, 0.12) ? 1 : 0;
                f.encl[f.I(i, j, k)] = float(double(hits) / ND);
            }
    return f;
}

struct Fit2Opts {
    V3 down{0, 0, -1}, sideHint{}, bodySide{};
    bool haveSide = false, haveBody = false, seat = false;
    char axis = 'y';
    double cut = 0.6, clear = 0.001, enclosed = 0.8, wallW = 0.02, target = 0.85, maxScale = 2.0;
    const char* dumpPath = nullptr;
    void Parse(int argc, char** argv, int first) {
        for (int i = first; i < argc; ++i) {
            double a, b, c;
            if (std::strncmp(argv[i], "dump=", 5) == 0) dumpPath = argv[i] + 5;
            else if (std::sscanf(argv[i], "down=%lf,%lf,%lf", &a, &b, &c) == 3) down = Unit({a, b, c});
            else if (std::sscanf(argv[i], "side=%lf,%lf,%lf", &a, &b, &c) == 3) sideHint = Unit({a, b, c}), haveSide = true;
            else if (std::sscanf(argv[i], "body=%lf,%lf,%lf", &a, &b, &c) == 3) bodySide = Unit({a, b, c}), haveBody = true;
            else if (std::strncmp(argv[i], "axis=", 5) == 0) axis = argv[i][5];
            else if (std::sscanf(argv[i], "cut=%lf", &a) == 1) cut = a;
            else if (std::sscanf(argv[i], "clear=%lf", &a) == 1) clear = a;
            else if (std::sscanf(argv[i], "enclosed=%lf", &a) == 1) enclosed = a;
            else if (std::sscanf(argv[i], "wall=%lf", &a) == 1) wallW = a;
            else if (std::sscanf(argv[i], "target=%lf", &a) == 1) target = a;
            else if (std::sscanf(argv[i], "maxscale=%lf", &a) == 1) maxScale = a;
            else if (std::strcmp(argv[i], "seat=1") == 0) seat = true;
        }
    }
};

struct Fit2Result {
    bool ok = false;
    Q q;
    V3 p;
    double cost = 0, share = 0, wallShare = 0, tipToEndMm = 0;
    size_t cavity = 0;
    V3 cc, ax[3];               // the cavity's centre and axes: [0] its length toward the closed end, [2] its thinnest
    double ext[3] = {};         // the cavity's extents along them
    double need[3] = {};        // the weapon's inserted part along the axes it goes on: length, height, thickness
};

std::vector<Tri> GameTris(const Mesh& m, std::vector<V3>& pts) {
    std::vector<Tri> tris;
    for (const Prim& p : m)
        for (int t = 0; t < p.nt(); ++t) {
            Tri q{Game(p.P(p.idx[3 * t])), Game(p.P(p.idx[3 * t + 1])), Game(p.P(p.idx[3 * t + 2])), {}};
            q.n = Unit(Cross(q.b - q.a, q.c - q.a));
            if (Len(q.n) == 0) continue;
            tris.push_back(q);
            for (V3 v : {q.a, q.b, q.c}) pts.push_back(v);
        }
    return tris;
}

Fit2Result Fit2Core(const std::vector<Tri>& tris, const std::vector<V3>& hp, const std::vector<V3>& wp,
                    const Fit2Opts& o) {
    Fit2Result r;
    Grid g;
    g.Build(tris, 0.01);
    V3 lo{1e9, 1e9, 1e9}, hi{-1e9, -1e9, -1e9};
    for (const V3& v : hp) {
        lo = {std::min(lo.x, v.x), std::min(lo.y, v.y), std::min(lo.z, v.z)};
        hi = {std::max(hi.x, v.x), std::max(hi.y, v.y), std::max(hi.z, v.z)};
    }
    const V3 margin{0.02, 0.02, 0.02};
    const Field fld = BuildField(g, lo - margin, hi + margin);

    // the weapon: the inserted part (from the tip back `cut` of its length), its tip, its extents
    auto along = [&](V3 p) { return o.axis == 'z' ? p.z : p.y; };
    auto height = [&](V3 p) { return o.axis == 'z' ? p.y : p.z; };   // guns: their height; blades: the edge's way
    double wlo = 1e9, whi = -1e9;
    for (const V3& p : wp) wlo = std::min(wlo, along(p)), whi = std::max(whi, along(p));
    std::vector<V3> inPart, outPart;
    for (size_t i = 0; i < wp.size(); i += std::max<size_t>(1, wp.size() / 3000)) {
        if (along(wp[i]) > whi - o.cut * (whi - wlo)) inPart.push_back(wp[i]);
        else outPart.push_back(wp[i]);
    }
    V3 tipLocal{}, inCentre{};
    {
        int n = 0;
        for (const V3& p : wp)
            if (along(p) > whi - 0.03 * (whi - wlo)) tipLocal = tipLocal + p, ++n;
        tipLocal = tipLocal * (1.0 / std::max(1, n));
        double hl = 1e9, hh = -1e9, xl = 1e9, xh = -1e9;
        for (const V3& p : inPart) {
            inCentre = inCentre + p;
            hl = std::min(hl, height(p)), hh = std::max(hh, height(p));
            xl = std::min(xl, p.x), xh = std::max(xh, p.x);
        }
        inCentre = inCentre * (1.0 / inPart.size());
        r.need[0] = o.cut * (whi - wlo), r.need[1] = hh - hl, r.need[2] = xh - xl;
    }
    // the cavity: the enclosed voxels clear of the walls; its axes, the long one toward the closed end
    std::vector<V3> cav;
    for (int i = 0; i < fld.n[0]; ++i)
        for (int j = 0; j < fld.n[1]; ++j)
            for (int k = 0; k < fld.n[2]; ++k)
                if (fld.encl[fld.I(i, j, k)] >= o.enclosed && fld.dist[fld.I(i, j, k)] > o.clear)
                    cav.push_back({fld.lo.x + (i + 0.5) * fld.h, fld.lo.y + (j + 0.5) * fld.h, fld.lo.z + (k + 0.5) * fld.h});
    r.cavity = cav.size();
    if (o.dumpPath) {                   // the cavity's voxels and the holster's points, float32 xyzw (w: 1 cavity)
        FILE* f = std::fopen(o.dumpPath, "wb");
        for (const V3& v : cav) {
            const float row[4] = {float(v.x), float(v.y), float(v.z), 1.0f};
            std::fwrite(row, 4, 4, f);
        }
        for (size_t i = 0; i < hp.size(); i += 3) {
            const float row[4] = {float(hp[i].x), float(hp[i].y), float(hp[i].z), 0.0f};
            std::fwrite(row, 4, 4, f);
        }
        std::fclose(f);
    }
    if (cav.size() < 20) return r;
    V3 ax[3];
    Pca(cav, r.cc, ax);
    const V3 u = Dot(ax[0], o.down) >= 0 ? ax[0] : ax[0] * -1.0;
    r.ax[0] = u, r.ax[1] = ax[1], r.ax[2] = Unit(Cross(u, ax[1]));
    double cuMax = -1e9;
    for (int k = 0; k < 3; ++k) {
        double a = 1e9, b = -1e9;
        for (const V3& p : cav) a = std::min(a, Dot(p - r.cc, r.ax[k])), b = std::max(b, Dot(p - r.cc, r.ax[k]));
        r.ext[k] = b - a + fld.h;
        if (k == 0) cuMax = b;
    }
    const V3 closed = r.cc + u * cuMax;

    // starts: the insertion axis along the cavity, the weapon's thickness (x) across its thinnest way, either way
    // round (the side hint picks)
    struct Start {
        Q q;
        V3 p;
    };
    std::vector<Start> starts;
    for (int pass = 0; pass < 2 && starts.empty(); ++pass)
        for (double sg : {1.0, -1.0}) {
            const V3 X = r.ax[2] * sg;
            V3 Xw, Yw, Zw;
            if (o.axis == 'z') Zw = u, Xw = X, Yw = Cross(Zw, Xw);
            else Yw = u, Xw = X, Zw = Cross(Xw, Yw);
            const Q q = QFromBasis(Xw, Yw, Zw);
            if (pass == 0 && o.haveSide && Dot(QRot(q, {1, 0, 0}), o.sideHint) < 0) continue;
            starts.push_back({q, r.cc - QRot(q, inCentre)});
        }
    auto evalPose = [&](Q q, V3 p, double* enclosedShare, double* wallShare) {
        double e = 0;
        int inside = 0, walls = 0;
        for (const V3& w : inPart) {
            double d, en;
            fld.At(p + QRot(q, w), d, en);
            const double miss = std::max(0.0, o.enclosed - en);
            e += miss * miss;
            if (d < o.clear) e += o.wallW * (o.clear - d) * (o.clear - d) / (o.clear * o.clear), ++walls;
            if (en >= o.enclosed) ++inside;
        }
        e /= inPart.size();
        double eo = 0;
        for (const V3& w : outPart) {
            double d, en;
            fld.At(p + QRot(q, w), d, en);
            if (d < o.clear) eo += o.wallW * (o.clear - d) * (o.clear - d) / (o.clear * o.clear), ++walls;
        }
        e += eo / std::max<size_t>(1, outPart.size());
        const V3 tip = p + QRot(q, tipLocal);
        const double gap = std::max(0.0, Dot(closed - tip, u));
        e += 0.5 * gap * gap;
        if (enclosedShare) *enclosedShare = double(inside) / inPart.size();
        if (wallShare) *wallShare = double(walls) / (inPart.size() + outPart.size());
        return e;
    };
    double bestE = 1e30;
    if (o.seat) {
        // SEATED: no search. The weapon straight down the cavity's axis, its tip SEAT_GAP short of the closed end,
        // its inserted part centred on the cavity's own middle over the depths it fills (the holster was stretched
        // to its section, so centred is inside)
        const double SEAT_GAP = 0.004;
        const Start& s0 = starts.front();
        V3 p = closed - u * SEAT_GAP - QRot(s0.q, tipLocal);
        double d0 = 1e9, d1 = -1e9, a1 = 1e9, b1 = -1e9, a2 = 1e9, b2 = -1e9;
        for (const V3& w : inPart) {
            const V3 v = p + QRot(s0.q, w) - r.cc;
            d0 = std::min(d0, Dot(v, u)), d1 = std::max(d1, Dot(v, u));
            a1 = std::min(a1, Dot(v, r.ax[1])), b1 = std::max(b1, Dot(v, r.ax[1]));
            a2 = std::min(a2, Dot(v, r.ax[2])), b2 = std::max(b2, Dot(v, r.ax[2]));
        }
        double c1 = 0, c2 = 0;
        int n = 0;
        for (const V3& v : cav) {
            const double d = Dot(v - r.cc, u);
            if (d < d0 || d > d1) continue;
            c1 += Dot(v - r.cc, r.ax[1]), c2 += Dot(v - r.cc, r.ax[2]), ++n;
        }
        if (n) c1 /= n, c2 /= n;
        p = p + r.ax[1] * (c1 - 0.5 * (a1 + b1)) + r.ax[2] * (c2 - 0.5 * (a2 + b2));
        r.q = s0.q, r.p = p;
        bestE = evalPose(r.q, r.p, nullptr, nullptr);
    }
    for (const Start& s0 : starts)
        for (double du : {-0.02, 0.0, 0.02}) {
            if (o.seat) break;
            auto pose = [&](const double* x, Q& q, V3& p) {
                q = QMul(QFromVec({x[3], x[4], x[5]}), s0.q);
                p = s0.p + u * du + V3{x[0], x[1], x[2]};
            };
            auto cost = [&](const double* x) {
                Q q;
                V3 p;
                pose(x, q, p);
                return evalPose(q, p, nullptr, nullptr) + 1e-4 * (x[3] * x[3] + x[4] * x[4] + x[5] * x[5]);
            };
            double x[6] = {0, 0, 0, 0, 0, 0};
            double e = Nelder(cost, x, 6, 0.01, 1200);
            e = Nelder(cost, x, 6, 0.004, 1200);
            e = Nelder(cost, x, 6, 0.001, 800);
            if (e < bestE) bestE = e, pose(x, r.q, r.p);
        }
    r.cost = bestE;
    evalPose(r.q, r.p, &r.share, &r.wallShare);
    r.tipToEndMm = 1e3 * Dot(closed - (r.p + QRot(r.q, tipLocal)), u);
    r.ok = true;
    return r;
}

void PrintFit(const Fit2Result& r, const char* extra) {
    std::printf("{\"p\": [%.6f, %.6f, %.6f], \"q\": [%.6f, %.6f, %.6f, %.6f], \"cost\": %.3g, \"inPocket\": %.3f, "
                "\"outInWalls\": %.3f, \"tipToEndMm\": %.1f, \"holsterLengthMm\": %.1f, \"cavityVoxels\": %zu, "
                "\"cavityMm\": [%.1f, %.1f, %.1f], \"needMm\": [%.1f, %.1f, %.1f]%s}\n",
                r.p.x, r.p.y, r.p.z, r.q.x, r.q.y, r.q.z, r.q.w, r.cost, r.share, r.wallShare, r.tipToEndMm,
                1e3 * r.ext[0], r.cavity, 1e3 * r.ext[0], 1e3 * r.ext[1], 1e3 * r.ext[2], 1e3 * r.need[0],
                1e3 * r.need[1], 1e3 * r.need[2], extra);
}

// options: down=x,y,z (the closed end's way), axis=y|z, cut=F, side=x,y,z (the weapon's +x), enclosed=E (the
// enclosure that counts as inside, default 0.8), wall=W (the weight against touching the walls, default 0.02: a weapon
// may graze the inside of a tight holster, nobody sees it; poking out of it shows as a point no longer enclosed),
// dump=PATH (the cavity and the holster as float32 xyzw)
int CmdFit2(int argc, char** argv) {
    std::vector<V3> hp;
    const std::vector<Tri> tris = GameTris(LoadMesh(argv[2]), hp);
    Fit2Opts o;
    o.Parse(argc, argv, 4);
    const Fit2Result r = Fit2Core(tris, hp, LoadPoints(argv[3]), o);
    if (!r.ok) {
        std::printf("{\"error\": \"no cavity\", \"voxels\": %zu}\n", r.cavity);
        return 1;
    }
    PrintFit(r, "");
    return 0;
}

// THE GAME'S HOLSTERS ARE FOR SMALLER GUNS. The gang's holsters are cut for a compact pistol; the player's handguns
// are bigger (a Unity's front is 78 mm tall, a Nova's 96 by 63). fitscale makes the holster's copy for one weapon: the
// holster stretched along its cavity's axes until the weapon's inserted part fits (from the sizes first, then 8% more
// a round while less than `target` of it is enclosed, at most `maxscale`), about its top (so it still hangs from its
// strap) and its body-side face (so it grows away from the body, body=x,y,z the way to the body), then fit2 in it.
//   fitscale HOLSTER.bin WEAPON.xyz OUT.bin [fit2 options] body=x,y,z target=F maxscale=S
int CmdFitScale(int argc, char** argv) {
    const Mesh src = LoadMesh(argv[2]);
    const std::vector<V3> wp = LoadPoints(argv[3]);
    Fit2Opts o;
    o.Parse(argc, argv, 5);
    std::vector<V3> hp0;
    const std::vector<Tri> tris0 = GameTris(src, hp0);
    Fit2Result r = Fit2Core(tris0, hp0, wp, o);
    if (!r.ok) {
        std::printf("{\"error\": \"no cavity\"}\n");
        return 1;
    }
    // the frame the stretch is along, fixed from the holster as it is
    const V3 e[3] = {r.ax[0], r.ax[1], r.ax[2]};
    V3 A = r.cc;
    {
        double top = 1e9, bodyFace = 0;
        const bool towardPlus = o.haveBody && Dot(e[2], o.bodySide) > 0;
        double b2lo = 1e9, b2hi = -1e9;
        for (const V3& p : hp0) {
            top = std::min(top, Dot(p - r.cc, e[0]));
            b2lo = std::min(b2lo, Dot(p - r.cc, e[2])), b2hi = std::max(b2hi, Dot(p - r.cc, e[2]));
        }
        bodyFace = o.haveBody ? (towardPlus ? b2hi : b2lo) : 0.0;
        A = r.cc + e[0] * top + e[2] * bodyFace;
    }
    double s[3] = {1, 1, 1};
    const double room = 2 * 0.002;
    s[0] = std::max(1.0, (r.need[0] + 0.005) / r.ext[0]);
    s[1] = std::max(1.0, (r.need[1] + room) / r.ext[1]);
    s[2] = std::max(1.0, (r.need[2] + room) / r.ext[2]);
    Mesh out = src;
    auto stretch = [&](Mesh& m) {
        m = src;
        for (Prim& p : m)
            for (int v = 0; v < p.nv(); ++v) {
                const V3 P = Game(p.P(v)) - A;
                const V3 N = Game({p.nrm[3 * v], p.nrm[3 * v + 1], p.nrm[3 * v + 2]});
                const V3 T = Game({p.tan[4 * v], p.tan[4 * v + 1], p.tan[4 * v + 2]});
                V3 P2 = A, N2{}, T2{};
                for (int k = 0; k < 3; ++k) {
                    P2 = P2 + e[k] * (s[k] * Dot(P, e[k]));
                    N2 = N2 + e[k] * (Dot(N, e[k]) / s[k]);
                    T2 = T2 + e[k] * (Dot(T, e[k]) * s[k]);
                }
                const V3 gp = Gltf(P2), gn = Gltf(Unit(N2)), gt = Gltf(Unit(T2));
                p.pos[3 * v] = float(gp.x), p.pos[3 * v + 1] = float(gp.y), p.pos[3 * v + 2] = float(gp.z);
                p.nrm[3 * v] = float(gn.x), p.nrm[3 * v + 1] = float(gn.y), p.nrm[3 * v + 2] = float(gn.z);
                p.tan[4 * v] = float(gt.x), p.tan[4 * v + 1] = float(gt.y), p.tan[4 * v + 2] = float(gt.z);
            }
    };
    int rounds = 0;
    for (;; ++rounds) {
        stretch(out);
        std::vector<V3> hp;
        const std::vector<Tri> tris = GameTris(out, hp);
        r = Fit2Core(tris, hp, wp, o);
        if (!r.ok) break;
        if (r.share >= o.target || rounds >= 6) break;
        bool grew = false;
        for (int k = 1; k < 3; ++k)
            if (s[k] * 1.08 <= o.maxScale) s[k] *= 1.08, grew = true;
        if (r.tipToEndMm < -3.0 && s[0] * 1.05 <= o.maxScale) s[0] *= 1.05, grew = true;
        if (!grew) break;
    }
    SaveMesh(argv[4], out);
    char extra[200];
    std::snprintf(extra, sizeof extra, ", \"scale\": [%.3f, %.3f, %.3f], \"rounds\": %d", s[0], s[1], s[2], rounds);
    PrintFit(r, extra);
    return r.ok ? 0 : 1;
}

// ---------------------------------------------------------------- sheathfit: a whole knife in the chest sheath

// The reference's chest knife is a grip alone (no blade: nothing to show through the sheath), so its pose says where
// the handle goes and nothing about the blade. The port's copy is the whole knife. sheathfit keeps the guard where the
// pose has it and puts the blade inside the sheath:
//   - the knife turned about its guard (the least turn) so its blade runs down the sheath's long axis;
//   - then, seen flat on the sheath (its length and width), the blade's outline laid inside the sheath's: which way
//     round its edge faces (the blade's tip is on its spine side; the sheath's on its straight side), a small tilt
//     about the guard, a shift across and a little up out of it, the ones that leave the least of the blade outside
//     (each move costs a little, so the knife stays as the pose has it where a move gains nothing);
//   - centred through the sheath's thickness;
//   - and only if the blade still does not fit, the sheath stretched from its throat along its length and across its
//     width, the least that takes the blade (`margin` all round).
// The sheath's body is its piece of the largest extent (bands and clip are the others); its axes that piece's principal
// ones (length, width, thickness). SHEATH.bin positions are glTF axes; KNIFE.xyz is float32 xyz in the knife's frame
// (game axes: origin at the guard, +z to the tip, +y the edge); the pose (frame -> model, game axes) is
// px,py,pz,qi,qj,qk,qr. Blade: z > cut.
//   ownshell sheathfit SHEATH.bin KNIFE.xyz OUT.bin pose=... [cut=0.02] [margin=0.002]

struct P2 {
    double x = 0, y = 0;
};

// convex hull, counter-clockwise (Andrew's monotone chain)
std::vector<P2> Hull(std::vector<P2> p) {
    std::sort(p.begin(), p.end(), [](P2 a, P2 b) { return a.x < b.x || (a.x == b.x && a.y < b.y); });
    auto cross = [](P2 o, P2 a, P2 b) { return (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x); };
    std::vector<P2> h(2 * p.size());
    size_t k = 0;
    for (size_t i = 0; i < p.size(); ++i) {
        while (k >= 2 && cross(h[k - 2], h[k - 1], p[i]) <= 0) --k;
        h[k++] = p[i];
    }
    for (size_t i = p.size() - 1, lo = k + 1; i-- > 0;) {
        while (k >= lo && cross(h[k - 2], h[k - 1], p[i]) <= 0) --k;
        h[k++] = p[i];
    }
    h.resize(k - 1);
    return h;
}

// how far p is outside the convex polygon (counter-clockwise), by its farthest edge line; negative inside
double Outside(const std::vector<P2>& h, P2 p) {
    double s = -1e18;
    for (size_t i = 0; i < h.size(); ++i) {
        const P2 a = h[i], b = h[(i + 1) % h.size()];
        const double ex = b.x - a.x, ey = b.y - a.y, l = std::sqrt(ex * ex + ey * ey);
        if (l < 1e-12) continue;
        s = std::max(s, ((p.x - a.x) * ey - (p.y - a.y) * ex) / l);
    }
    return s;
}

int CmdSheathFit(int argc, char** argv) {
    Mesh m = LoadMesh(argv[2]);
    const std::vector<V3> knife = LoadPoints(argv[3]);
    double pose[7] = {0, 0, 0, 0, 0, 0, 1}, cut = 0.02, margin = 0.002;
    for (int i = 5; i < argc; ++i) {
        const std::string a = argv[i];
        if (a.rfind("pose=", 0) == 0)
            std::sscanf(a.c_str() + 5, "%lf,%lf,%lf,%lf,%lf,%lf,%lf", &pose[0], &pose[1], &pose[2], &pose[3], &pose[4],
                        &pose[5], &pose[6]);
        else if (a.rfind("cut=", 0) == 0) cut = std::atof(a.c_str() + 4);
        else if (a.rfind("margin=", 0) == 0) margin = std::atof(a.c_str() + 7);
    }
    const V3 p0{pose[0], pose[1], pose[2]};
    const Q q0{pose[3], pose[4], pose[5], pose[6]};
    const double pi = 3.14159265358979323846;

    // the sheath's body and its frame (game axes): d down the sheath, w across, t through
    const std::vector<Piece> pcs = Pieces(m);
    int body = -1;
    double bodySize = -1;
    for (size_t i = 0; i < pcs.size(); ++i) {
        V3 lo{1e9, 1e9, 1e9}, hi{-1e9, -1e9, -1e9};
        for (int v : pcs[i].verts) {
            const V3 g = Game(m[pcs[i].prim].P(v));
            lo = {std::min(lo.x, g.x), std::min(lo.y, g.y), std::min(lo.z, g.z)};
            hi = {std::max(hi.x, g.x), std::max(hi.y, g.y), std::max(hi.z, g.z)};
        }
        if (Len(hi - lo) > bodySize) bodySize = Len(hi - lo), body = int(i);
    }
    std::vector<V3> sb;
    for (int v : pcs[body].verts) sb.push_back(Game(m[pcs[body].prim].P(v)));
    V3 c, ax[3];
    Pca(sb, c, ax);
    V3 d = ax[0], w = ax[1], t = ax[2];
    if (d.z > 0) d = d * -1.0;                       // down the sheath
    t = Unit(Cross(d, w));
    w = Unit(Cross(t, d));
    auto frameOf = [&](V3 g) { return V3{Dot(g - c, d), Dot(g - c, w), Dot(g - c, t)}; };
    double top = 1e9, bottom = -1e9, sw0 = 1e9, sw1 = -1e9, st0 = 1e9, st1 = -1e9;
    std::vector<P2> outline;
    for (const V3& g : sb) {
        const V3 f = frameOf(g);
        top = std::min(top, f.x), bottom = std::max(bottom, f.x);
        sw0 = std::min(sw0, f.y), sw1 = std::max(sw1, f.y), st0 = std::min(st0, f.z), st1 = std::max(st1, f.z);
        outline.push_back({f.x, f.y});
    }
    const std::vector<P2> hull = Hull(outline);

    // the knife's blade onto the sheath's axis about its guard (the frame's origin)
    const V3 blade0 = QRot(q0, {0, 0, 1});
    const V3 axis = Cross(blade0, d);
    const double turn = std::atan2(Len(axis), Dot(blade0, d));
    const Q qa = QMul(QFromVec(Unit(axis) * turn), q0);
    const V3 edge = QRot(qa, {0, 1, 0});
    const double roll0 = std::atan2(Dot(edge, t), Dot(edge, w));   // the roll that lays the edge onto +w
    std::vector<V3> blade;
    for (const V3& k : knife)
        if (k.z > cut) blade.push_back(k);

    // seen flat: which way round, a tilt about the guard, a shift across -- the least outside
    struct Try {
        Q q;
        double tilt = 0, shift = 0, rise = 0, out = 1e18, cost = 1e18;
        int side = 0;
    } best;
    for (int side = 0; side < 2; ++side) {
        const Q qr = QMul(QFromVec(d * -(roll0 + side * pi)), qa);
        for (int ti = -24; ti <= 24; ++ti) {
            const double tilt = ti * 0.5 * pi / 180;
            const Q q = QMul(QFromVec(t * tilt), qr);
            std::vector<P2> pts;
            for (const V3& k : blade) {
                const V3 f = frameOf(QRot(q, k) + p0);
                pts.push_back({f.x, f.y});
            }
            for (int ri = 0; ri <= 20; ++ri)
                for (int si = -30; si <= 30; ++si) {
                    const double sh = si * 0.001, rise = ri * 0.001;
                    double out = -1e18;
                    for (const P2& q2 : pts) out = std::max(out, Outside(hull, {q2.x - rise, q2.y + sh}));
                    const double cost =
                        std::max(out, -margin) + 2e-5 * std::abs(ti) + 0.02 * std::fabs(sh) + 0.05 * rise;
                    if (cost < best.cost) best = {q, tilt, sh, rise, out, cost, side};
                }
        }
    }
    const Q q = best.q;
    // through the thickness: the blade's middle onto the sheath's
    double bt0 = 1e9, bt1 = -1e9;
    for (const V3& k : blade) {
        const V3 f = frameOf(QRot(q, k) + p0);
        bt0 = std::min(bt0, f.z), bt1 = std::max(bt1, f.z);
    }
    const double tShift = (st0 + st1) / 2 - (bt0 + bt1) / 2;
    const V3 p = p0 + w * best.shift + t * tShift - d * best.rise;

    // the stretch from the throat, only as much as the blade still needs
    const P2 T2{top, (sw0 + sw1) / 2};
    std::vector<P2> pts;
    for (const V3& k : blade) {
        const V3 f = frameOf(QRot(q, k) + p);
        pts.push_back({f.x, f.y});
    }
    double sLen = 1, sW = 1;
    if (best.out > -margin) {
        double area = 1e18;
        for (int li = 0; li <= 70; ++li)
            for (int wi = 0; wi <= 60; ++wi) {
                const double sl = 1 + li * 0.005, sw = 1 + wi * 0.01;
                if (sl * sw >= area) continue;
                double out = -1e18;
                for (const P2& q2 : pts)
                    out = std::max(out, Outside(hull, {T2.x + (q2.x - T2.x) / sl, T2.y + (q2.y - T2.y) / sw}));
                if (out <= -margin / std::max(sl, sw)) area = sl * sw, sLen = sl, sW = sw;
            }
    }
    double sT = 1;
    {
        const double ht = (st1 - st0) / 2, bh = (bt1 - bt0) / 2;
        if (ht > 1e-4) sT = std::max(1.0, (bh + margin) / ht);
    }

    // every kept piece stretched (glTF axes in the file: through the game-axis frame)
    const V3 T = c + d * top + w * T2.y + t * ((st0 + st1) / 2);
    const double s[3] = {sLen, sW, sT};
    const V3 e[3] = {d, w, t};
    auto toGltf = [](V3 g) { return V3{g.x, g.z, -g.y}; };
    for (Prim& pr : m)
        for (int i = 0; i < pr.nv(); ++i) {
            const V3 g = Game(pr.P(i)) - T;
            V3 y = T, n{}, tg{};
            const V3 gn = Game({pr.nrm[3 * i], pr.nrm[3 * i + 1], pr.nrm[3 * i + 2]});
            const V3 gt = Game({pr.tan[4 * i], pr.tan[4 * i + 1], pr.tan[4 * i + 2]});
            for (int k = 0; k < 3; ++k) {
                y = y + e[k] * (Dot(g, e[k]) * s[k]);
                n = n + e[k] * (Dot(gn, e[k]) / s[k]);      // inverse transpose of a scale
                tg = tg + e[k] * (Dot(gt, e[k]) * s[k]);
            }
            const V3 Y = toGltf(y), N = toGltf(Unit(n)), TG = toGltf(Unit(tg));
            pr.pos[3 * i] = float(Y.x), pr.pos[3 * i + 1] = float(Y.y), pr.pos[3 * i + 2] = float(Y.z);
            pr.nrm[3 * i] = float(N.x), pr.nrm[3 * i + 1] = float(N.y), pr.nrm[3 * i + 2] = float(N.z);
            pr.tan[4 * i] = float(TG.x), pr.tan[4 * i + 1] = float(TG.y), pr.tan[4 * i + 2] = float(TG.z);
        }
    SaveMesh(argv[4], m);
    std::printf("{\"p\": [%.6f, %.6f, %.6f], \"q\": [%.6f, %.6f, %.6f, %.6f], \"turnDeg\": %.2f, \"flipped\": %d, "
                "\"tiltDeg\": %.1f, \"shiftMm\": %.1f, \"riseMm\": %.1f, \"thickShiftMm\": %.1f, \"outsideMm\": %.1f, "
                "\"stretch\": [%.3f, %.3f, %.3f]}\n",
                p.x, p.y, p.z, q.x, q.y, q.z, q.w, turn * 57.29578, best.side, best.tilt * 57.29578, 1e3 * best.shift,
                1e3 * best.rise, 1e3 * tShift, 1e3 * best.out, sLen, sW, sT);
    return 0;
}

}  // namespace

int main(int argc, char** argv) {
    if (argc < 3) {
        std::fprintf(stderr, "ownshell mirror|refit|parts|drop|keep|pieces|fit|fit2|fitscale|place|sheathfit ...\n");
        return 2;
    }
    const std::string cmd = argv[1];
    if (cmd == "mirror" && argc >= 4) return CmdMirror(argv[2], argv[3]);
    if (cmd == "refit" && argc >= 6)
        return CmdRefit(argv[2], argv[3], argv[4], argv[5], argc >= 7 && std::string(argv[6]) == "rigid");
    if (cmd == "parts") return CmdParts(argv[2]);
    if (cmd == "drop" && argc >= 5) return CmdDrop(argv[2], argv[3], argv[4]);
    if (cmd == "keep" && argc >= 5) return CmdDrop(argv[2], argv[3], argv[4], true);
    if (cmd == "pieces" && argc >= 5) return CmdPieces(argv[2], argv[3], argv[4]);
    if (cmd == "fit" && argc >= 4) return CmdFit(argc, argv);
    if (cmd == "fit2" && argc >= 4) return CmdFit2(argc, argv);
    if (cmd == "fitscale" && argc >= 5) return CmdFitScale(argc, argv);
    if (cmd == "place" && argc >= 8) return CmdPlace(argc, argv);
    if (cmd == "sheathfit" && argc >= 6) return CmdSheathFit(argc, argv);
    std::fprintf(stderr, "bad command\n");
    return 2;
}
