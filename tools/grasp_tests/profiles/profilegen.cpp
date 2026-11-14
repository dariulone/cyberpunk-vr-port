// profilegen.exe PACK OUT.inc: the radius profiles of the game's long round props (bottles, cans, cups), from their cooked
// meshes' positions (pack.py lays them out), for the grasp (src/Natives/HeldBody.cpp): a prop's collider is a capsule round
// the whole of it, which has no neck, so where a bottle's neck is and how thin, and how thick its body really is, come from
// the mesh it is drawn with.
//
// Per mesh: its LOD0 positions decoded (Short4N: the value / 32767, times the quantization's scale, plus its offset), its
// bounds, its long axis (the longest extent, at least 1.2 times the section's, the section round within 25%), and the
// largest distance from that axis (through the middle of the section's bounds) in each of kSlices slices along it, read
// along every edge of its triangles every 2 mm: a straight side has vertices only at its two ends, and a slice between
// them would otherwise see only what is inside (a wine bottle's punt). Meshes that are no long round thing are left out.
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace {
constexpr int kSlices = 16;
constexpr float kRound = 0.25f, kLong = 1.2f;

struct Mesh {
    uint64_t hash = 0;
    std::string path;
    float scale[3]{}, offset[3]{};
    std::vector<float> p;   // x y z
    std::vector<uint32_t> tri;  // three a triangle, into p
};

template <typename T>
bool Get(FILE* f, T& v) { return std::fread(&v, sizeof(T), 1, f) == 1; }

bool Load(const char* file, std::vector<Mesh>& out) {
    FILE* f = std::fopen(file, "rb");
    if (!f) return false;
    char magic[4];
    uint32_t count = 0;
    if (std::fread(magic, 1, 4, f) != 4 || std::memcmp(magic, "GPRF", 4) != 0 || !Get(f, count)) { std::fclose(f); return false; }
    for (uint32_t m = 0; m < count; ++m) {
        Mesh mesh;
        uint16_t len = 0;
        if (!Get(f, mesh.hash) || !Get(f, len)) break;
        mesh.path.resize(len);
        if (std::fread(mesh.path.data(), 1, len, f) != len) break;
        if (std::fread(mesh.scale, 4, 3, f) != 3 || std::fread(mesh.offset, 4, 3, f) != 3) break;
        uint32_t chunks = 0;
        if (!Get(f, chunks)) break;
        for (uint32_t c = 0; c < chunks; ++c) {
            uint32_t n = 0;
            if (!Get(f, n)) break;
            std::vector<int16_t> raw(size_t(n) * 4);
            if (std::fread(raw.data(), 8, n, f) != n) break;
            const uint32_t first = uint32_t(mesh.p.size() / 3);
            for (uint32_t i = 0; i < n; ++i)
                for (int k = 0; k < 3; ++k) {
                    const float v = std::max(-1.0f, raw[size_t(i) * 4 + k] / 32767.0f);
                    mesh.p.push_back(v * mesh.scale[k] + mesh.offset[k]);
                }
            uint32_t ni = 0;
            if (!Get(f, ni)) break;
            std::vector<uint16_t> idx(ni);
            if (ni && std::fread(idx.data(), 2, ni, f) != ni) break;
            for (uint32_t i = 0; i + 2 < ni; i += 3)
                if (idx[i] < n && idx[i + 1] < n && idx[i + 2] < n)
                    for (int k = 0; k < 3; ++k) mesh.tri.push_back(first + idx[i + k]);
        }
        out.push_back(std::move(mesh));
    }
    std::fclose(f);
    return true;
}

struct Profile {
    int axis = 2;
    float lo = 0, hi = 0, cb = 0, cc = 0;
    float r[kSlices]{};
    bool neck = false;
    int neckEnd = 0;    // +1 the high end, -1 the low end
};

bool Make(const Mesh& m, Profile& pr, float* ext) {
    const size_t n = m.p.size() / 3;
    if (n < 8) return false;
    float lo[3] = {1e9f, 1e9f, 1e9f}, hi[3] = {-1e9f, -1e9f, -1e9f};
    for (size_t i = 0; i < n; ++i)
        for (int k = 0; k < 3; ++k) { lo[k] = std::min(lo[k], m.p[i * 3 + k]); hi[k] = std::max(hi[k], m.p[i * 3 + k]); }
    for (int k = 0; k < 3; ++k) ext[k] = hi[k] - lo[k];
    const int a = int(std::max_element(ext, ext + 3) - ext), b = (a + 1) % 3, c = (a + 2) % 3;
    const float section = std::max(ext[b], ext[c]);
    if (!(section > 0.004f) || std::fabs(ext[b] - ext[c]) > kRound * section || ext[a] < kLong * section) return false;
    pr.axis = a;
    pr.lo = lo[a];
    pr.hi = hi[a];
    pr.cb = 0.5f * (lo[b] + hi[b]);
    pr.cc = 0.5f * (lo[c] + hi[c]);
    float r[kSlices];
    for (float& v : r) v = -1.0f;
    const float w = (pr.hi - pr.lo) / kSlices;
    const auto take = [&](const float* p) {
        const int k = std::clamp(int((p[a] - pr.lo) / w), 0, kSlices - 1);
        const float db = p[b] - pr.cb, dc = p[c] - pr.cc;
        r[k] = std::max(r[k], std::sqrt(db * db + dc * dc));
    };
    for (size_t i = 0; i < n; ++i) take(&m.p[i * 3]);
    constexpr float kStep = 0.002f;
    for (size_t t = 0; t + 2 < m.tri.size(); t += 3)
        for (int e = 0; e < 3; ++e) {
            const float* u = &m.p[size_t(m.tri[t + e]) * 3];
            const float* v = &m.p[size_t(m.tri[t + (e + 1) % 3]) * 3];
            const float d[3] = {v[0] - u[0], v[1] - u[1], v[2] - u[2]};
            const float len = std::sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
            const int steps = std::min(400, int(len / kStep));
            for (int s = 1; s < steps; ++s) {
                const float f = float(s) / float(steps);
                const float q[3] = {u[0] + d[0] * f, u[1] + d[1] * f, u[2] + d[2] * f};
                take(q);
            }
        }
    // empty slices (a long straight side has no points between its ends): the straight line between their neighbours
    int first = -1, last = -1;
    for (int k = 0; k < kSlices; ++k)
        if (r[k] >= 0.0f) { if (first < 0) first = k; last = k; }
    if (first < 0) return false;
    for (int k = 0; k < first; ++k) r[k] = r[first];
    for (int k = last + 1; k < kSlices; ++k) r[k] = r[last];
    for (int k = first + 1; k < last; ++k) {
        if (r[k] >= 0.0f) continue;
        int e = k + 1;
        while (r[e] < 0.0f) ++e;
        r[k] = r[k - 1] + (r[e] - r[k - 1]) / float(e - k + 1);
    }
    std::copy_n(r, kSlices, pr.r);
    // a neck, as the grasp reads one (GraspPartOf): a run of slices under 62% of the widest at one end, 2 cm long at least
    // and no more than half of it, and the other end's three slices over 85%
    const float widest = *std::max_element(r, r + kSlices);
    for (int s = 1; s >= -1 && !pr.neck; s -= 2) {
        int run = 0;
        while (run < kSlices && r[s > 0 ? kSlices - 1 - run : run] < 0.62f * widest) ++run;
        float other = 0.0f;
        for (int k = 0; k < 3; ++k) other = std::max(other, r[s > 0 ? k : kSlices - 1 - k]);
        if (run >= 1 && run * w >= 0.02f && run <= kSlices / 2 && other > 0.85f * widest) { pr.neck = true; pr.neckEnd = s; }
    }
    return true;
}
}  // namespace

int main(int argc, char** argv) {
    if (argc < 3) { std::printf("profilegen PACK OUT.inc\n"); return 2; }
    std::vector<Mesh> meshes;
    if (!Load(argv[1], meshes)) { std::printf("cannot read %s\n", argv[1]); return 1; }
    FILE* o = std::fopen(argv[2], "w");
    if (!o) return 1;
    std::fprintf(o, "// Generated by tools/grasp_tests/profiles/profilegen.cpp from the game's cooked meshes (tools/grasp_tests/profiles/README.md):\n"
                    "// do not edit. Per mesh: its depot path's FNV1a64, its long axis (0 x, 1 y, 2 z), that axis's low and high end, the\n"
                    "// middle of its section on the other two axes (in x y z order after it), and the radius round the axis in %d slices.\n",
                 kSlices);
    int kept = 0, necks = 0;
    for (const Mesh& m : meshes) {
        Profile pr;
        float ext[3]{};
        const bool ok = Make(m, pr, ext);
        const char* name = std::strrchr(m.path.c_str(), '\\');
        name = name ? name + 1 : m.path.c_str();
        std::printf("%-48s %5.3f %5.3f %5.3f %s", name, ext[0], ext[1], ext[2], ok ? "" : "  (not long and round)\n");
        if (!ok) continue;
        ++kept;
        necks += pr.neck;
        std::printf(" axis %d%s r:", pr.axis, pr.neck ? (pr.neckEnd > 0 ? " NECK high" : " NECK low") : "");
        for (float v : pr.r) std::printf(" %.3f", v);
        std::printf("\n");
        std::fprintf(o, "{0x%016llXull, %d, %.4ff, %.4ff, %.4ff, %.4ff, {", static_cast<unsigned long long>(m.hash), pr.axis, pr.lo, pr.hi,
                     pr.cb, pr.cc);
        for (int k = 0; k < kSlices; ++k) std::fprintf(o, "%s%.4ff", k ? ", " : "", pr.r[k]);
        std::fprintf(o, "}},  // %s\n", m.path.c_str());
    }
    std::fclose(o);
    std::printf("%d of %zu meshes long and round, %d with a neck\n", kept, meshes.size(), necks);
    return 0;
}
