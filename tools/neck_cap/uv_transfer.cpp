// uv_transfer CAP BODY OUT PU PV K: each cap vertex takes a texture coordinate and a normal of the body's skin.
//
// CAP:  uint32 n, then n * (x, y, z) float32 -- the cap's positions, in the body mesh's own space.
// BODY: uint32 m, then m * (x, y, z, u, v, u1, v1, nx, ny, nz, tx, ty, tz, tw) float32 -- every vertex of the body
//       mesh, with its second texture coordinate (u1, v1) and its tangent (w the bitangent's sign); then uint32 t,
//       then t * (a, b, c) uint32 -- its triangles, as indices into those vertices.
// PU PV: the patch -- a texture coordinate of plain skin on the body's front island, below the neck opening.
// K:    the patch's texture coordinates per metre of the cap.
// OUT:  n * (u, v, u1, v1, nx, ny, nz, tx, ty, tz, tw) float32, in the cap's vertex order; on stderr what was matched.
//
// The second texture coordinate goes with the first: at the rim the body's, inside the body's at the patch (one value;
// the skin shader reads it too, and the game's cone, drawn only in shadows, had one point for all of it).
// THE TANGENT GOES WITH THE NORMAL: a new normal under the cone's old tangents left them far from perpendicular
// (|N.T| up to 0.95), and the cap was lit darker than the skin round it whatever its normal was. At the rim the
// body's tangent, inside the direction the patch's U grows along (its first axis); then made perpendicular to the
// normal.
//
// Called by make_neck_cap.py (the uvs step), which reads and writes the glTF files.
//
// The cap is a low cone across the body's neck opening: its rim lies on the body, its inside over the hole, where the
// body has no surface. At the rim each vertex takes the texture coordinate and the normal of the body's closest point
// (interpolated over that triangle), so the skin runs on across the seam. Inside, the closest body point is the rim
// again, and mapping the whole cap that way spread it along the opening's edge: on the texture that edge is the
// island's border, padded with smeared texels, and the cap's triangles across the hole drew long streaks between
// opposite sides of it. So away from the rim the cap is a flat patch of plain skin instead -- its two widest axes
// mapped around PU PV at K per metre -- with the normal of the skin at that patch (the upper chest), so it is lit as
// that skin is. The two meet over kBand from the rim.
//
// One UV island only. The body's UV seam runs down the chest, and points matched across it drew a zigzag. The island is
// the one of the body vertex nearest the cap's centre (the reference): only triangles whose three corners lie within
// kIsland of the reference's UV are matched, so a cap vertex over the other island takes the nearest skin of this one.
#include <algorithm>
#include <cfloat>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <vector>

namespace {
constexpr int kBody = 14;   // x y z u v u1 v1 nx ny nz tx ty tz tw
constexpr int kOut = 11;    // u v u1 v1 nx ny nz tx ty tz tw
constexpr float kIsland = 0.12f;   // UV distance from the reference that still counts as its island (the cap is ~10 cm)
constexpr float kBand = 0.02f;     // metres from the rim over which the body's skin gives way to the patch
constexpr float kPatchNormal = 0.02f;  // UV radius round the patch whose body normals light the cap

struct V3 {
    float x, y, z;
};
V3 operator-(V3 a, V3 b) { return {a.x - b.x, a.y - b.y, a.z - b.z}; }
V3 operator+(V3 a, V3 b) { return {a.x + b.x, a.y + b.y, a.z + b.z}; }
V3 operator*(V3 a, float s) { return {a.x * s, a.y * s, a.z * s}; }
float Dot(V3 a, V3 b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
V3 Normalized(V3 a) {
    const float l = std::sqrt(Dot(a, a));
    return l > 0 ? a * (1.0f / l) : a;
}

// The closest point of triangle abc to p, as barycentric weights (Ericson, Real-Time Collision Detection 5.1.5).
void Closest(V3 p, V3 a, V3 b, V3 c, float w[3]) {
    const V3 ab = b - a, ac = c - a, ap = p - a;
    const float d1 = Dot(ab, ap), d2 = Dot(ac, ap);
    if (d1 <= 0 && d2 <= 0) { w[0] = 1; w[1] = 0; w[2] = 0; return; }
    const V3 bp = p - b;
    const float d3 = Dot(ab, bp), d4 = Dot(ac, bp);
    if (d3 >= 0 && d4 <= d3) { w[0] = 0; w[1] = 1; w[2] = 0; return; }
    const float vc = d1 * d4 - d3 * d2;
    if (vc <= 0 && d1 >= 0 && d3 <= 0) {
        const float v = d1 / (d1 - d3);
        w[0] = 1 - v; w[1] = v; w[2] = 0;
        return;
    }
    const V3 cp = p - c;
    const float d5 = Dot(ab, cp), d6 = Dot(ac, cp);
    if (d6 >= 0 && d5 <= d6) { w[0] = 0; w[1] = 0; w[2] = 1; return; }
    const float vb = d5 * d2 - d1 * d6;
    if (vb <= 0 && d2 >= 0 && d6 <= 0) {
        const float v = d2 / (d2 - d6);
        w[0] = 1 - v; w[1] = 0; w[2] = v;
        return;
    }
    const float va = d3 * d6 - d5 * d4;
    if (va <= 0 && (d4 - d3) >= 0 && (d5 - d6) >= 0) {
        const float v = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        w[0] = 0; w[1] = 1 - v; w[2] = v;
        return;
    }
    const float denom = 1.0f / (va + vb + vc);
    w[1] = vb * denom;
    w[2] = vc * denom;
    w[0] = 1 - w[1] - w[2];
}

// The eigenvectors of a symmetric 3x3 matrix (Jacobi rotations), as the columns of v, by eigenvalue, largest first.
void Eigen(double a[3][3], double v[3][3]) {
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j) v[i][j] = i == j ? 1.0 : 0.0;
    for (int sweep = 0; sweep < 50; ++sweep) {
        const double off = a[0][1] * a[0][1] + a[0][2] * a[0][2] + a[1][2] * a[1][2];
        if (off < 1e-30) break;
        for (int p = 0; p < 2; ++p)
            for (int q = p + 1; q < 3; ++q) {
                if (std::fabs(a[p][q]) < 1e-30) continue;
                const double theta = (a[q][q] - a[p][p]) / (2 * a[p][q]);
                const double t = (theta >= 0 ? 1.0 : -1.0) / (std::fabs(theta) + std::sqrt(theta * theta + 1));
                const double c = 1 / std::sqrt(t * t + 1), s = t * c;
                for (int k = 0; k < 3; ++k) {
                    const double akp = a[k][p], akq = a[k][q];
                    a[k][p] = c * akp - s * akq;
                    a[k][q] = s * akp + c * akq;
                }
                for (int k = 0; k < 3; ++k) {
                    const double apk = a[p][k], aqk = a[q][k];
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
                for (int k = 0; k < 3; ++k) {
                    const double vkp = v[k][p], vkq = v[k][q];
                    v[k][p] = c * vkp - s * vkq;
                    v[k][q] = s * vkp + c * vkq;
                }
            }
    }
    int order[3] = {0, 1, 2};
    std::sort(order, order + 3, [&](int x, int y) { return a[x][x] > a[y][y]; });
    double sorted[3][3];
    for (int k = 0; k < 3; ++k)
        for (int c = 0; c < 3; ++c) sorted[k][c] = v[k][order[c]];
    for (int k = 0; k < 3; ++k)
        for (int c = 0; c < 3; ++c) v[k][c] = sorted[k][c];
}
}  // namespace

int main(int argc, char** argv) {
    if (argc != 7) {
        std::fprintf(stderr, "usage: uv_transfer CAP BODY OUT PU PV K\n");
        return 2;
    }
    const float pu = std::strtof(argv[4], nullptr), pv = std::strtof(argv[5], nullptr), scale = std::strtof(argv[6], nullptr);
    std::vector<float> cap, body;
    std::vector<uint32_t> tris;
    uint32_t n = 0, m = 0, t = 0;
    FILE* f = std::fopen(argv[1], "rb");
    bool ok = f && std::fread(&n, 4, 1, f) == 1;
    if (ok) {
        cap.resize(static_cast<size_t>(n) * 3);
        ok = std::fread(cap.data(), 4, cap.size(), f) == cap.size();
    }
    if (f) std::fclose(f);
    f = std::fopen(argv[2], "rb");
    ok = ok && f && std::fread(&m, 4, 1, f) == 1;
    if (ok) {
        body.resize(static_cast<size_t>(m) * kBody);
        ok = std::fread(body.data(), 4, body.size(), f) == body.size() && std::fread(&t, 4, 1, f) == 1;
    }
    if (ok) {
        tris.resize(static_cast<size_t>(t) * 3);
        ok = std::fread(tris.data(), 4, tris.size(), f) == tris.size();
    }
    if (f) std::fclose(f);
    if (!ok || n == 0 || m == 0 || t == 0 || *std::max_element(tris.begin(), tris.end()) >= m || !(scale > 0)) {
        std::fprintf(stderr, "cannot read the inputs\n");
        return 1;
    }
    auto at = [&](uint32_t j) { return &body[static_cast<size_t>(j) * kBody]; };
    auto pos = [&](uint32_t j) { const float* q = at(j); return V3{q[0], q[1], q[2]}; };
    V3 centre{0, 0, 0};
    for (uint32_t i = 0; i < n; ++i) centre = centre + V3{cap[i * 3], cap[i * 3 + 1], cap[i * 3 + 2]} * (1.0f / n);
    uint32_t reference = 0;
    float best = FLT_MAX;
    for (uint32_t j = 0; j < m; ++j) {
        const V3 d = pos(j) - centre;
        if (Dot(d, d) < best) {
            best = Dot(d, d);
            reference = j;
        }
    }
    const float ru = at(reference)[3], rv = at(reference)[4];
    auto onIsland = [&](uint32_t j) {
        const float du = at(j)[3] - ru, dv = at(j)[4] - rv;
        return du * du + dv * dv <= kIsland * kIsland;
    };
    std::vector<uint32_t> island;
    for (uint32_t k = 0; k < t; ++k)
        if (onIsland(tris[k * 3]) && onIsland(tris[k * 3 + 1]) && onIsland(tris[k * 3 + 2])) island.push_back(k);
    if (island.empty()) {
        std::fprintf(stderr, "no triangle on the reference's island\n");
        return 1;
    }

    // The patch's normal and second texture coordinate: the body's skin round PU PV, on the same island.
    V3 patchNormal{0, 0, 0};
    float patchUv1[2] = {0, 0};
    int patchVertices = 0;
    for (uint32_t j = 0; j < m; ++j) {
        const float du = at(j)[3] - pu, dv = at(j)[4] - pv;
        if (du * du + dv * dv <= kPatchNormal * kPatchNormal && onIsland(j)) {
            patchNormal = patchNormal + V3{at(j)[7], at(j)[8], at(j)[9]};
            patchUv1[0] += at(j)[5];
            patchUv1[1] += at(j)[6];
            ++patchVertices;
        }
    }
    if (patchVertices == 0) {
        std::fprintf(stderr, "no body vertex round the patch %.4f %.4f\n", pu, pv);
        return 1;
    }
    patchNormal = Normalized(patchNormal);
    patchUv1[0] /= patchVertices;
    patchUv1[1] /= patchVertices;

    // The patch's axes: the cap's two widest directions (the cone is low, its third axis the shortest).
    double cov[3][3] = {}, axes[3][3];
    for (uint32_t i = 0; i < n; ++i) {
        const double d[3] = {cap[i * 3] - centre.x, cap[i * 3 + 1] - centre.y, cap[i * 3 + 2] - centre.z};
        for (int r = 0; r < 3; ++r)
            for (int c = 0; c < 3; ++c) cov[r][c] += d[r] * d[c];
    }
    Eigen(cov, axes);
    const V3 a1{static_cast<float>(axes[0][0]), static_cast<float>(axes[1][0]), static_cast<float>(axes[2][0])};
    const V3 a2{static_cast<float>(axes[0][1]), static_cast<float>(axes[1][1]), static_cast<float>(axes[2][1])};

    std::vector<float> out(static_cast<size_t>(n) * kOut);
    double worst = 0.0;
    int rim = 0;
    for (uint32_t i = 0; i < n; ++i) {
        const V3 p{cap[i * 3], cap[i * 3 + 1], cap[i * 3 + 2]};
        float near = FLT_MAX, bw[3] = {1, 0, 0};
        uint32_t hit = island[0];
        for (uint32_t k : island) {
            const uint32_t* c = &tris[static_cast<size_t>(k) * 3];
            float w[3];
            const V3 a = pos(c[0]), b = pos(c[1]), cc = pos(c[2]);
            Closest(p, a, b, cc, w);
            const V3 q = a * w[0] + b * w[1] + cc * w[2];
            const V3 d = p - q;
            if (Dot(d, d) < near) {
                near = Dot(d, d);
                hit = k;
                std::copy(w, w + 3, bw);
            }
        }
        float skin[10] = {};   // u v u1 v1 nx ny nz tx ty tz at the closest point
        float sign = 1.0f;     // the bitangent's sign of the corner that weighs most
        float heaviest = -1.0f;
        for (int v = 0; v < 3; ++v) {
            const float* q = at(tris[static_cast<size_t>(hit) * 3 + v]);
            for (int c = 0; c < 10; ++c) skin[c] += q[3 + c] * bw[v];
            if (bw[v] > heaviest) {
                heaviest = bw[v];
                sign = q[13] < 0 ? -1.0f : 1.0f;
            }
        }
        const float dist = std::sqrt(near);
        const float x = std::clamp(dist / kBand, 0.0f, 1.0f), blend = x * x * (3 - 2 * x);
        const V3 d = p - centre;
        const float patch[4] = {pu + scale * Dot(d, a1), pv + scale * Dot(d, a2), patchUv1[0], patchUv1[1]};
        float* r = &out[static_cast<size_t>(i) * kOut];
        for (int c = 0; c < 4; ++c) r[c] = skin[c] + (patch[c] - skin[c]) * blend;
        const V3 normal = Normalized(Normalized(V3{skin[4], skin[5], skin[6]}) * (1 - blend) + patchNormal * blend);
        r[4] = normal.x;
        r[5] = normal.y;
        r[6] = normal.z;
        V3 tangent = Normalized(V3{skin[7], skin[8], skin[9]}) * (1 - blend) + a1 * blend;
        tangent = tangent - normal * Dot(tangent, normal);
        if (Dot(tangent, tangent) < 1e-8f) {   // along the normal: any perpendicular
            const V3 other = std::fabs(normal.x) < 0.9f ? V3{1, 0, 0} : V3{0, 1, 0};
            tangent = other - normal * Dot(other, normal);
        }
        tangent = Normalized(tangent);
        r[7] = tangent.x;
        r[8] = tangent.y;
        r[9] = tangent.z;
        r[10] = sign;
        if (blend < 1) ++rim;
        worst = std::fmax(worst, static_cast<double>(dist));
    }
    f = std::fopen(argv[3], "wb");
    if (!f || std::fwrite(out.data(), 4, out.size(), f) != out.size()) {
        std::fprintf(stderr, "cannot write %s\n", argv[3]);
        return 1;
    }
    std::fclose(f);
    std::fprintf(stderr,
                 "%u cap vertices (%d by the rim) on %zu of %u body triangles (island round uv %.4f, %.4f), farthest %.4f m;"
                 " patch %.4f %.4f x %.2f/m, uv1 %.4f %.4f, normal (%.3f %.3f %.3f) of %d body vertices\n",
                 n, rim, island.size(), t, ru, rv, worst, pu, pv, scale, patchUv1[0], patchUv1[1], patchNormal.x,
                 patchNormal.y, patchNormal.z, patchVertices);
    return 0;
}
