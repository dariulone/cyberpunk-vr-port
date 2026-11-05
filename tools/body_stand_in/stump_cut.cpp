// stump_cut CAP BODY OUT MARGIN: which triangles of the body mesh stand above the neck cap and would hide it.
//
// CAP:  uint32 n, n * (x, y, z) float32, uint32 t, t * (a, b, c) uint32 -- the cap's vertices and triangles.
// BODY: uint32 m, m * (x, y, z) float32, uint32 k, k * (a, b, c) uint32 -- the body's vertices and triangles
//       (every primitive's, in order, as indices into those vertices).
// MARGIN: metres a body vertex must stand above the cap to count as above it.
// OUT:  k bytes, 1 for a triangle to cut; on stderr how many.
//
// Called by make_body_stand_in.py (the mesh step). Both meshes are in the glTF export's space (Y up). The game
// removes the body's triangles under its own first-person neck cone by the garment assembly, which leaves the
// neck opening open for the cap. The stand-in is a garment itself, so nothing removes them from it: its neck stump
// stood over the cap, a dark disc in the middle of it. A point is over the cap where a vertical line through it meets
// the cap's surface lower than the point by more than MARGIN; a triangle is cut when any of its corners, edge middles
// or centre is (a triangle spanning the opening has its corners on the rim). Body skin round the opening lies under
// the cap's rim or level with it and stays.
#include <algorithm>
#include <cfloat>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <vector>

namespace {
bool ReadMesh(const char* path, std::vector<float>& xyz, std::vector<uint32_t>& tris) {
    FILE* f = std::fopen(path, "rb");
    if (!f) return false;
    uint32_t n = 0, t = 0;
    bool ok = std::fread(&n, 4, 1, f) == 1;
    if (ok) {
        xyz.resize(static_cast<size_t>(n) * 3);
        ok = std::fread(xyz.data(), 4, xyz.size(), f) == xyz.size() && std::fread(&t, 4, 1, f) == 1;
    }
    if (ok) {
        tris.resize(static_cast<size_t>(t) * 3);
        ok = std::fread(tris.data(), 4, tris.size(), f) == tris.size();
    }
    std::fclose(f);
    return ok && n && t && *std::max_element(tris.begin(), tris.end()) < n;
}
}  // namespace

int main(int argc, char** argv) {
    if (argc != 5) {
        std::fprintf(stderr, "usage: stump_cut CAP BODY OUT MARGIN\n");
        return 2;
    }
    std::vector<float> cap, body;
    std::vector<uint32_t> capTris, bodyTris;
    if (!ReadMesh(argv[1], cap, capTris) || !ReadMesh(argv[2], body, bodyTris)) {
        std::fprintf(stderr, "cannot read the inputs\n");
        return 1;
    }
    const float margin = std::strtof(argv[4], nullptr);
    // The cap's surface height under (x, z): the highest of its triangles there; -FLT_MAX outside its footprint.
    auto capTop = [&](float px, float pz) {
        float top = -FLT_MAX;
        for (size_t k = 0; k < capTris.size(); k += 3) {
            const float* a = &cap[static_cast<size_t>(capTris[k]) * 3];
            const float* b = &cap[static_cast<size_t>(capTris[k + 1]) * 3];
            const float* c = &cap[static_cast<size_t>(capTris[k + 2]) * 3];
            // Barycentric weights of (px, pz) in the triangle's projection on the ground plane (x, z).
            const float d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
            if (d > -1e-12f && d < 1e-12f) continue;   // seen edge-on from above
            const float w0 = ((b[2] - c[2]) * (px - c[0]) + (c[0] - b[0]) * (pz - c[2])) / d;
            const float w1 = ((c[2] - a[2]) * (px - c[0]) + (a[0] - c[0]) * (pz - c[2])) / d;
            const float w2 = 1.0f - w0 - w1;
            if (w0 < 0 || w1 < 0 || w2 < 0) continue;
            top = std::max(top, w0 * a[1] + w1 * b[1] + w2 * c[1]);
        }
        return top;
    };
    const size_t m = body.size() / 3;
    // A triangle is cut when any of seven points on it -- its corners, its edges' middles, its centre -- stands
    // over the cap: a triangle spanning the opening has its corners on the rim, outside the cap's footprint, and
    // still lies over the cap's middle.
    std::vector<uint8_t> cut(bodyTris.size() / 3, 0);
    size_t cutCount = 0;
    for (size_t k = 0; k < cut.size(); ++k) {
        const float* p[3] = {&body[static_cast<size_t>(bodyTris[k * 3]) * 3], &body[static_cast<size_t>(bodyTris[k * 3 + 1]) * 3],
                             &body[static_cast<size_t>(bodyTris[k * 3 + 2]) * 3]};
        static const float kWeights[7][3] = {{1, 0, 0}, {0, 1, 0}, {0, 0, 1}, {.5f, .5f, 0}, {0, .5f, .5f}, {.5f, 0, .5f},
                                             {1 / 3.f, 1 / 3.f, 1 / 3.f}};
        for (const auto& w : kWeights) {
            float q[3];
            for (int c = 0; c < 3; ++c) q[c] = p[0][c] * w[0] + p[1][c] * w[1] + p[2][c] * w[2];
            const float top = capTop(q[0], q[2]);
            if (top > -FLT_MAX && q[1] > top + margin) {
                cut[k] = 1;
                ++cutCount;
                break;
            }
        }
    }
    FILE* f = std::fopen(argv[3], "wb");
    if (!f || std::fwrite(cut.data(), 1, cut.size(), f) != cut.size()) {
        std::fprintf(stderr, "cannot write %s\n", argv[3]);
        return 1;
    }
    std::fclose(f);
    std::fprintf(stderr, "%zu of %zu body triangles stand over the cap (margin %.4f m, %zu vertices); cut\n", cutCount,
                 cut.size(), margin, m);
    return 0;
}
