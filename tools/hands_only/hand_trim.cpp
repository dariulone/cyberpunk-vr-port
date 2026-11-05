// hand_trim: cut an arm mesh primitive at the wrist and close the cut.
//
//   hand_trim.exe in.bin out.bin
//
// The heavy geometry of the hands-only meshes (tools/hands_only/make_hand_meshes.py does the file work around it).
//
// IN (little endian):
//   u32 vertexCount, u32 indexCount, u32 sides (1 or 2: a mesh of one arm, or of both; a vertex belongs to the side
//   whose hand joint is nearest), then per side:
//   f32 hand[3]      the hand joint in the mesh's bind pose
//   f32 axis[3]      unit, from the forearm joint toward the hand
//   f32 cut          the plane: a vertex with t = dot(p - hand, axis) < -cut is on the forearm side
//   f32 band         a boundary loop is the wrist's own when its points lie within 2.5 cm past the plane and `band`
//                    short of it, and it rings the forearm
//   f32 positions[vertexCount * 3], f32 normals[vertexCount * 3], u32 indices[indexCount]
// OUT:
//   u32 keptIndexCount, u32 newVertexCount, u32 loops
//   u32 kept[keptIndexCount]          the triangles left, then the cap's, in one index list (every index < vertexCount)
//   per new vertex: u32 slot (the vertex it is written over), u32 source (the vertex whose skinning, colour and
//                   texture coordinates it takes), f32 position[3], f32 normal[3]
//
// THE VERTEX COUNT STAYS. WolvenKit's import onto the game's mesh keeps the mesh's per-vertex data (garment
// support) and refuses a primitive with more vertices than it had. So the cap's vertices are written over vertices
// the cut left unused (the forearm's), and a loop with too few of those left is closed on its own rim vertices.
//
// THE CUT. A triangle with any corner on the forearm side goes (whole triangles, no clipping: the ring the cut
// leaves runs along the mesh's own edges, a millimetre or two below the plane). Vertices are welded by position for
// the topology -- a mesh splits its vertices along texture seams, and a seam is not a boundary.
//
// THE CAP. Every boundary loop of the kept triangles that lies on the cut (near the plane, long enough to go round a
// wrist, ringing the forearm) is closed by a fan: rim vertices copied from the loop's own (same skinning, so the cap
// stays sewn to the arm under any pose) with the cap's normal, and a centre vertex at the loop's centroid that takes
// its skinning from the loop vertex nearest the centroid -- or, short of free vertices, a fan from the rim vertex
// nearest the centroid over the rim itself. The fan faces out of the arm: along -axis. Loops elsewhere (the
// openings the mesh has on purpose, the seams between its chunks) are left as they are.
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <map>
#include <unordered_map>
#include <vector>

namespace {
struct V3 { float x, y, z; };
V3 operator-(V3 a, V3 b) { return {a.x - b.x, a.y - b.y, a.z - b.z}; }
V3 operator+(V3 a, V3 b) { return {a.x + b.x, a.y + b.y, a.z + b.z}; }
V3 operator*(V3 a, float s) { return {a.x * s, a.y * s, a.z * s}; }
float Dot(V3 a, V3 b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
V3 Cross(V3 a, V3 b) { return {a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x}; }
float Len(V3 a) { return std::sqrt(Dot(a, a)); }

bool ReadAll(const char* path, std::vector<uint8_t>& out) {
    FILE* f = std::fopen(path, "rb");
    if (!f) return false;
    std::fseek(f, 0, SEEK_END);
    const long n = std::ftell(f);
    std::fseek(f, 0, SEEK_SET);
    out.resize(static_cast<size_t>(n));
    const bool ok = std::fread(out.data(), 1, out.size(), f) == out.size();
    std::fclose(f);
    return ok;
}

struct NewVertex { uint32_t slot, source; V3 position, normal; };
}

int main(int argc, char** argv) {
    if (argc != 3) {
        std::fprintf(stderr, "usage: hand_trim in.bin out.bin\n");
        return 2;
    }
    const bool debug = std::getenv("HAND_TRIM_DEBUG") != nullptr;   // every boundary loop, to stderr
    std::vector<uint8_t> in;
    if (!ReadAll(argv[1], in) || in.size() < 40) {
        std::fprintf(stderr, "cannot read %s\n", argv[1]);
        return 1;
    }
    const uint8_t* p = in.data();
    uint32_t vc, ic, sides;
    std::memcpy(&vc, p, 4);
    std::memcpy(&ic, p + 4, 4);
    std::memcpy(&sides, p + 8, 4);
    if (sides < 1 || sides > 2) {
        std::fprintf(stderr, "bad side count\n");
        return 1;
    }
    V3 hands[2]{}, axes[2]{};
    size_t at = 12;
    for (uint32_t s = 0; s < sides; ++s) {
        std::memcpy(&hands[s], p + at, 12);
        std::memcpy(&axes[s], p + at + 12, 12);
        at += 24;
    }
    float cut, band;
    std::memcpy(&cut, p + at, 4);
    std::memcpy(&band, p + at + 4, 4);
    at += 8;
    const size_t need = at + size_t(vc) * 24 + size_t(ic) * 4;
    if (in.size() < need || ic % 3) {
        std::fprintf(stderr, "bad input size\n");
        return 1;
    }
    std::vector<V3> pos(vc), nrm(vc);
    std::memcpy(pos.data(), p + at, size_t(vc) * 12);
    std::memcpy(nrm.data(), p + at + size_t(vc) * 12, size_t(vc) * 12);
    std::vector<uint32_t> idx(ic);
    std::memcpy(idx.data(), p + at + size_t(vc) * 24, size_t(ic) * 4);

    // each vertex's side (the nearest hand joint) and its plane coordinate on that side
    std::vector<uint8_t> side(vc, 0);
    std::vector<float> t(vc);
    for (uint32_t i = 0; i < vc; ++i) {
        if (sides == 2 && Len(pos[i] - hands[1]) < Len(pos[i] - hands[0])) side[i] = 1;
        t[i] = Dot(pos[i] - hands[side[i]], axes[side[i]]);
    }

    // weld by position (0.05 mm grid) for the topology
    std::vector<uint32_t> weld(vc);
    {
        std::unordered_map<uint64_t, uint32_t> grid;
        grid.reserve(vc * 2);
        for (uint32_t i = 0; i < vc; ++i) {
            const auto q = [](float v) { return static_cast<int64_t>(std::llround(v * 20000.0)); };
            const uint64_t key = (uint64_t(q(pos[i].x) & 0x1FFFFF) << 42) | (uint64_t(q(pos[i].y) & 0x1FFFFF) << 21) |
                                 uint64_t(q(pos[i].z) & 0x1FFFFF);
            auto it = grid.find(key);
            if (it == grid.end()) {
                grid.emplace(key, i);
                weld[i] = i;
            } else {
                weld[i] = it->second;
            }
        }
    }

    // the cut: whole triangles with a corner on the forearm side go
    std::vector<uint32_t> kept;
    kept.reserve(ic);
    for (uint32_t k = 0; k < ic; k += 3) {
        const uint32_t a = idx[k], b = idx[k + 1], c = idx[k + 2];
        if (a >= vc || b >= vc || c >= vc) continue;
        if (t[a] < -cut || t[b] < -cut || t[c] < -cut) continue;
        kept.push_back(a);
        kept.push_back(b);
        kept.push_back(c);
    }

    // boundary edges of the kept triangles, on welded ids: an edge used once. Directed as the triangle has it,
    // so a loop follows the surface's own winding.
    std::map<std::pair<uint32_t, uint32_t>, int> count;
    for (size_t k = 0; k < kept.size(); k += 3) {
        for (int e = 0; e < 3; ++e) {
            uint32_t a = weld[kept[k + e]], b = weld[kept[k + (e + 1) % 3]];
            if (a == b) continue;
            count[{std::min(a, b), std::max(a, b)}]++;
        }
    }
    // next[a] = b along the boundary, with an original vertex index for each welded one
    std::unordered_map<uint32_t, uint32_t> next, original;
    for (size_t k = 0; k < kept.size(); k += 3) {
        for (int e = 0; e < 3; ++e) {
            const uint32_t ia = kept[k + e], ib = kept[k + (e + 1) % 3];
            const uint32_t a = weld[ia], b = weld[ib];
            if (a == b) continue;
            if (count[{std::min(a, b), std::max(a, b)}] != 1) continue;
            next[a] = b;
            original.emplace(a, ia);
            original.emplace(b, ib);
        }
    }

    std::vector<NewVertex> added;
    std::vector<uint32_t> cap;
    uint32_t loops = 0;
    // the vertices no kept triangle uses: the slots the cap's own vertices are written over
    std::vector<uint32_t> freeSlots;
    {
        std::vector<uint8_t> referenced(vc, 0);
        for (uint32_t i : kept) referenced[i] = 1;
        for (uint32_t i = vc; i-- > 0;)
            if (!referenced[i]) freeSlots.push_back(i);
    }
    std::unordered_map<uint32_t, bool> used;
    for (const auto& [startId, unused] : next) {
        (void)unused;
        if (used[startId]) continue;
        std::vector<uint32_t> loop;
        uint32_t cur = startId;
        bool closed = false;
        for (size_t guard = 0; guard < next.size() + 1; ++guard) {
            if (used[cur]) {
                closed = cur == startId;
                break;
            }
            used[cur] = true;
            loop.push_back(cur);
            auto it = next.find(cur);
            if (it == next.end()) break;
            cur = it->second;
        }
        if (debug) std::fprintf(stderr, "loop %zu points, %s\n", loop.size(), closed ? "closed" : "open");
        if (!closed || loop.size() < 6) continue;
        // on the cut, and round a wrist (>= 8 cm of rim)
        float lo = 1e9f, hi = -1e9f, length = 0;
        V3 centre{0, 0, 0};
        const uint8_t loopSide = side[original[loop[0]]];
        const V3 hand = hands[loopSide], axis = axes[loopSide];
        for (size_t i = 0; i < loop.size(); ++i) {
            const V3 a = pos[original[loop[i]]], b = pos[original[loop[(i + 1) % loop.size()]]];
            const float ta = Dot(a - hand, axis);
            lo = std::min(lo, ta);
            hi = std::max(hi, ta);
            length += Len(b - a);
            centre = centre + a;
        }
        centre = centre * (1.0f / float(loop.size()));
        // a ring round the forearm: the loop's own plane (its area vector) square to the axis
        V3 ring{0, 0, 0};
        for (size_t i = 0; i < loop.size(); ++i)
            ring = ring + Cross(pos[original[loop[i]]] - centre, pos[original[loop[(i + 1) % loop.size()]]] - centre);
        const float facing = Len(ring) > 1e-12f ? std::fabs(Dot(ring, axis)) / Len(ring) : 0.0f;
        if (debug) std::fprintf(stderr, "  t %.4f .. %.4f, rim %.3f m, facing %.2f\n", lo, hi, length, facing);
        // the cut's ring runs along the mesh's own edges: up to kBelow past the plane, up to `band` short of it
        constexpr float kBelow = 0.025f;
        if (lo < -cut - kBelow || hi > -cut + band || length < 0.08f || facing < 0.6f) continue;
        ++loops;
        // the fan faces out of the arm, toward the elbow: -axis. The loop runs the way the kept surface winds it;
        // a fan over it in that order faces the other way from the surface's outside, so it is checked and turned.
        const V3 out = axis * -1.0f;
        size_t nearestAt = 0;
        float best = 1e9f;
        for (size_t i = 0; i < loop.size(); ++i) {
            const float d = Len(pos[original[loop[i]]] - centre);
            if (d < best) { best = d; nearestAt = i; }
        }
        const uint32_t nearest = original[loop[nearestAt]];
        // the rim in order, and the fan's apex: the cap's own vertices when there are slots for them, else the rim's
        std::vector<uint32_t> rim;
        uint32_t apex;
        V3 apexAt;
        if (freeSlots.size() >= loop.size() + 1) {
            apex = freeSlots.back();
            freeSlots.pop_back();
            added.push_back({apex, nearest, centre, out});
            apexAt = centre;
            for (uint32_t id : loop) {
                const uint32_t slot = freeSlots.back();
                freeSlots.pop_back();
                added.push_back({slot, original[id], pos[original[id]], out});
                rim.push_back(slot);
            }
        } else {
            // a fan from the rim vertex nearest the centre over the rest of the rim (a wrist ring is near convex)
            for (size_t i = 1; i < loop.size(); ++i) rim.push_back(original[loop[(nearestAt + i) % loop.size()]]);
            apex = nearest;
            apexAt = pos[nearest];
            rim.push_back(rim.front());     // closed below without the wrap-around triangle back to the apex
        }
        auto positionOf = [&](uint32_t i) {
            for (const auto& v : added)
                if (v.slot == i) return v.position;
            return pos[i];
        };
        V3 area{0, 0, 0};
        const size_t edges = (rim.size() > 0 && rim.back() == rim.front() && apex == nearest) ? rim.size() - 2
                                                                                             : rim.size();
        for (size_t i = 0; i < edges; ++i) {
            const uint32_t a = rim[i], b = rim[(i + 1) % rim.size()];
            area = area + Cross(positionOf(a) - apexAt, positionOf(b) - apexAt);
        }
        const bool flip = Dot(area, out) < 0;
        for (size_t i = 0; i < edges; ++i) {
            const uint32_t a = rim[i], b = rim[(i + 1) % rim.size()];
            cap.push_back(apex);
            cap.push_back(flip ? b : a);
            cap.push_back(flip ? a : b);
        }
    }

    kept.insert(kept.end(), cap.begin(), cap.end());
    FILE* f = std::fopen(argv[2], "wb");
    if (!f) return 1;
    const uint32_t head[3] = {uint32_t(kept.size()), uint32_t(added.size()), loops};
    std::fwrite(head, 4, 3, f);
    std::fwrite(kept.data(), 4, kept.size(), f);
    for (const auto& v : added) {
        std::fwrite(&v.slot, 4, 1, f);
        std::fwrite(&v.source, 4, 1, f);
        std::fwrite(&v.position, 4, 3, f);
        std::fwrite(&v.normal, 4, 3, f);
    }
    std::fclose(f);
    std::printf("kept %zu of %u triangles, %u cap loop(s), %zu new vertices\n", (kept.size() - cap.size()) / 3, ic / 3,
                loops, added.size());
    return 0;
}
