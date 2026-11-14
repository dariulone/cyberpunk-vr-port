// fit_boxes: each weapon's collision shape as a few boxes fitted to the game's own vertices (for
// tools/emit_weapon_shapes.py, which reads the meshes and writes the Lua; the numbers are made here).
//
// Usage: fit_boxes <points.bin> <boxes.txt> [most boxes] [least gain]
//
// points.bin, little-endian: "WPTS", u32 families; for each: u16 name length, the name, u32 chunks; for each chunk: f32
// scale x y z, f32 offset x y z, u32 vertex count, u32 stride, then the vertex stream's bytes (each vertex starts with its quantised
// position, s16 x y z of a PT_Short4N).
// boxes.txt: for each family a line "name count fill", then count lines "minX maxX minY maxY minZ maxZ" (metres, the gun's
// frame: origin at the grip, +Y along the barrel, +Z up); fill is the boxes' volume over the outline's.
//
// THE CUTS. One box round a whole gun is mostly air (a pistol's is an L in a rectangle). The box with the most to gain is
// cut in two, again and again: across any of the three axes, at any of 128 places along it, where the two halves' boxes
// leave out the most volume. Each half is boxed from its own vertices, except at the faces that are cuts: there the two
// halves meet, so a long face with vertices only at its ends (a barrel's side) has no gap where the cut went through it.
// It stops at the most boxes, or when no cut would leave out the least gain of the whole outline.
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace {
constexpr int kBins = 128;
constexpr float kThin = 0.008f;   // the least half size a box is counted (and kept) at

struct Box {
    float lo[3], hi[3];
};
struct Leaf {
    std::vector<int> pts;
    Box box;
    bool fixedLo[3], fixedHi[3];   // a face that is a cut: it stays where the cut put it
    // the best cut found for it
    bool cut = false;
    int axis = 0;
    float at = 0, gain = 0;
};

float Volume(const Box& b) {
    float v = 1;
    for (int k = 0; k < 3; ++k) v *= std::max(b.hi[k] - b.lo[k], 2 * kThin);
    return v;
}
void Empty(Box& b) {
    for (int k = 0; k < 3; ++k) b.lo[k] = 1e30f, b.hi[k] = -1e30f;
}
void Grow(Box& b, const float* p) {
    for (int k = 0; k < 3; ++k) b.lo[k] = std::min(b.lo[k], p[k]), b.hi[k] = std::max(b.hi[k], p[k]);
}
void Grow(Box& b, const Box& o) {
    for (int k = 0; k < 3; ++k) b.lo[k] = std::min(b.lo[k], o.lo[k]), b.hi[k] = std::max(b.hi[k], o.hi[k]);
}

// A half's box: its points' own, but at the faces that are cuts (its parent's, and the new one) where those are.
Box HalfBox(const Box& tight, const Leaf& parent, int axis, float at, bool lower) {
    Box b = tight;
    for (int k = 0; k < 3; ++k) {
        if (parent.fixedLo[k]) b.lo[k] = parent.box.lo[k];
        if (parent.fixedHi[k]) b.hi[k] = parent.box.hi[k];
    }
    if (lower) b.hi[axis] = at;
    else b.lo[axis] = at;
    return b;
}

void BestCut(Leaf& L, const std::vector<float>& P) {
    L.cut = false;
    L.gain = 0;
    if (L.pts.size() < 8) return;
    const float whole = Volume(L.box);
    for (int a = 0; a < 3; ++a) {
        const float lo = L.box.lo[a], hi = L.box.hi[a];
        const float span = hi - lo;
        if (span < 4 * kThin) continue;
        Box bins[kBins];
        int count[kBins] = {};
        for (auto& b : bins) Empty(b);
        for (int i : L.pts) {
            const float* p = &P[size_t(i) * 3];
            int bin = int((p[a] - lo) / span * kBins);
            bin = std::clamp(bin, 0, kBins - 1);
            Grow(bins[bin], p);
            ++count[bin];
        }
        Box below[kBins], above[kBins];
        int nBelow[kBins], nAbove[kBins];
        Box acc;
        Empty(acc);
        int n = 0;
        for (int i = 0; i < kBins; ++i) {
            if (count[i]) Grow(acc, bins[i]);
            n += count[i];
            below[i] = acc;
            nBelow[i] = n;
        }
        Empty(acc);
        n = 0;
        for (int i = kBins - 1; i >= 0; --i) {
            if (count[i]) Grow(acc, bins[i]);
            n += count[i];
            above[i] = acc;
            nAbove[i] = n;
        }
        for (int i = 0; i + 1 < kBins; ++i) {
            if (nBelow[i] < 4 || nAbove[i + 1] < 4) continue;
            const float at = lo + span * float(i + 1) / kBins;
            const Box l = HalfBox(below[i], L, a, at, true);
            const Box r = HalfBox(above[i + 1], L, a, at, false);
            const float gain = whole - Volume(l) - Volume(r);
            if (gain > L.gain) {
                L.gain = gain;
                L.cut = true;
                L.axis = a;
                L.at = at;
            }
        }
    }
}

std::vector<Box> Fit(const std::vector<float>& P, int most, float leastGain, float* fill) {
    const int n = int(P.size() / 3);
    Leaf root;
    Empty(root.box);
    root.pts.resize(size_t(n));
    for (int i = 0; i < n; ++i) {
        root.pts[size_t(i)] = i;
        Grow(root.box, &P[size_t(i) * 3]);
    }
    for (int k = 0; k < 3; ++k) root.fixedLo[k] = root.fixedHi[k] = false;
    const float outline = Volume(root.box);
    std::vector<Leaf> leaves;
    leaves.push_back(std::move(root));
    BestCut(leaves[0], P);
    while (int(leaves.size()) < most) {
        int best = -1;
        for (int i = 0; i < int(leaves.size()); ++i)
            if (leaves[size_t(i)].cut && (best < 0 || leaves[size_t(i)].gain > leaves[size_t(best)].gain)) best = i;
        if (best < 0 || leaves[size_t(best)].gain < leastGain * outline) break;
        Leaf parent = std::move(leaves[size_t(best)]);
        Leaf l, r;
        Box tl, tr;
        Empty(tl);
        Empty(tr);
        for (int i : parent.pts) {
            const float* p = &P[size_t(i) * 3];
            if (p[parent.axis] < parent.at) l.pts.push_back(i), Grow(tl, p);
            else r.pts.push_back(i), Grow(tr, p);
        }
        l.box = HalfBox(tl, parent, parent.axis, parent.at, true);
        r.box = HalfBox(tr, parent, parent.axis, parent.at, false);
        for (int k = 0; k < 3; ++k) {
            l.fixedLo[k] = r.fixedLo[k] = parent.fixedLo[k];
            l.fixedHi[k] = r.fixedHi[k] = parent.fixedHi[k];
        }
        l.fixedHi[parent.axis] = true;
        r.fixedLo[parent.axis] = true;
        BestCut(l, P);
        BestCut(r, P);
        leaves[size_t(best)] = std::move(l);
        leaves.push_back(std::move(r));
    }
    std::vector<Box> out;
    float sum = 0;
    for (const Leaf& L : leaves) {
        Box b = L.box;
        // the least half size: a box thinner than that is widened about its middle
        for (int k = 0; k < 3; ++k)
            if (b.hi[k] - b.lo[k] < 2 * kThin) {
                const float c = 0.5f * (b.lo[k] + b.hi[k]);
                b.lo[k] = c - kThin, b.hi[k] = c + kThin;
            }
        out.push_back(b);
        sum += Volume(b);
    }
    *fill = outline > 0 ? sum / outline : 0;
    // the order the Lua keeps: back to front along the barrel
    std::sort(out.begin(), out.end(), [](const Box& a, const Box& b) { return a.lo[1] + a.hi[1] < b.lo[1] + b.hi[1]; });
    return out;
}

template <typename T>
bool Get(FILE* f, T& v) {
    return std::fread(&v, sizeof(T), 1, f) == 1;
}
}  // namespace

int main(int argc, char** argv) {
    if (argc < 3) {
        std::fprintf(stderr, "usage: fit_boxes <points.bin> <boxes.txt> [most boxes] [least gain]\n");
        return 2;
    }
    const int most = argc > 3 ? std::max(1, std::atoi(argv[3])) : 10;
    const float leastGain = argc > 4 ? float(std::atof(argv[4])) : 0.015f;
    FILE* in = std::fopen(argv[1], "rb");
    if (!in) {
        std::fprintf(stderr, "cannot read %s\n", argv[1]);
        return 1;
    }
    char magic[4];
    uint32_t families = 0;
    if (std::fread(magic, 1, 4, in) != 4 || std::memcmp(magic, "WPTS", 4) || !Get(in, families)) {
        std::fprintf(stderr, "not a points file\n");
        return 1;
    }
    FILE* out = std::fopen(argv[2], "w");
    if (!out) {
        std::fprintf(stderr, "cannot write %s\n", argv[2]);
        return 1;
    }
    double fills = 0, boxes = 0;
    for (uint32_t fam = 0; fam < families; ++fam) {
        uint16_t len = 0;
        if (!Get(in, len)) return 1;
        std::string name(len, '\0');
        if (len && std::fread(name.data(), 1, len, in) != len) return 1;
        uint32_t chunks = 0;
        if (!Get(in, chunks)) return 1;
        std::vector<float> P;
        for (uint32_t c = 0; c < chunks; ++c) {
            float scale[3], offset[3];
            uint32_t count = 0, stride = 0;
            if (std::fread(scale, sizeof(float), 3, in) != 3 || std::fread(offset, sizeof(float), 3, in) != 3 || !Get(in, count) ||
                !Get(in, stride) || stride < 6)
                return 1;
            std::vector<uint8_t> raw(size_t(count) * stride);
            if (count && std::fread(raw.data(), 1, raw.size(), in) != raw.size()) return 1;
            for (uint32_t i = 0; i < count; ++i) {
                int16_t q[3];
                std::memcpy(q, &raw[size_t(i) * stride], sizeof(q));
                for (int k = 0; k < 3; ++k) P.push_back(float(q[k]) / 32767.0f * scale[k] + offset[k]);
            }
        }
        if (P.empty()) continue;
        float fill = 0;
        const std::vector<Box> fit = Fit(P, most, leastGain, &fill);
        std::fprintf(out, "%s %d %.4f\n", name.c_str(), int(fit.size()), fill);
        for (const Box& b : fit)
            std::fprintf(out, "%.4f %.4f %.4f %.4f %.4f %.4f\n", b.lo[0], b.hi[0], b.lo[1], b.hi[1], b.lo[2], b.hi[2]);
        fills += fill;
        boxes += double(fit.size());
    }
    std::fclose(out);
    std::fclose(in);
    std::printf("families %u, mean boxes %.1f, mean share of the outline they fill %.2f\n", families, families ? boxes / families : 0,
                families ? fills / families : 0);
    return 0;
}
