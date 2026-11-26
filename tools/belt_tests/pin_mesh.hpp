#pragma once
#include "pin_math.hpp"
#include <algorithm>
#include <array>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <stdexcept>
#include <vector>

// LOD0 triangles and an AABB tree for the actual prop surfaces. Queries use
// distance to the triangle surface, with inside/outside for closed body meshes.
struct PinMesh {
    struct Triangle {
        V a, b, c;
    };
    struct Node {
        V low, high;
        int begin = 0, end = 0, left = -1, right = -1;
    };
    std::vector<Triangle> triangles;
    std::vector<Node> nodes;
    bool solid = true;
    static double component(V v, int axis) {
        return axis == 0 ? v.x : axis == 1 ? v.y : v.z;
    }
    static V minimum(V a, V b) {
        return {std::min(a.x, b.x), std::min(a.y, b.y), std::min(a.z, b.z)};
    }
    static V maximum(V a, V b) {
        return {std::max(a.x, b.x), std::max(a.y, b.y), std::max(a.z, b.z)};
    }
    static V nearest(V p, const Triangle &t) {
        V ab = t.b - t.a, ac = t.c - t.a, ap = p - t.a;
        double d1 = dot(ab, ap), d2 = dot(ac, ap);
        if (d1 <= 0 && d2 <= 0)
            return t.a;
        V bp = p - t.b;
        double d3 = dot(ab, bp), d4 = dot(ac, bp);
        if (d3 >= 0 && d4 <= d3)
            return t.b;
        double vc = d1 * d4 - d3 * d2;
        if (vc <= 0 && d1 >= 0 && d3 <= 0)
            return t.a + ab * (d1 / (d1 - d3));
        V cp = p - t.c;
        double d5 = dot(ab, cp), d6 = dot(ac, cp);
        if (d6 >= 0 && d5 <= d6)
            return t.c;
        double vb = d5 * d2 - d1 * d6;
        if (vb <= 0 && d2 >= 0 && d6 <= 0)
            return t.a + ac * (d2 / (d2 - d6));
        double va = d3 * d6 - d5 * d4;
        if (va <= 0 && d4 >= d3 && d5 >= d6)
            return t.b + (t.c - t.b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
        double sum = 1 / (va + vb + vc);
        return t.a + ab * (vb * sum) + ac * (vc * sum);
    }
    int build(int begin, int end) {
        Node n;
        n.begin = begin;
        n.end = end;
        n.low = {1e9, 1e9, 1e9};
        n.high = {-1e9, -1e9, -1e9};
        for (int i = begin; i < end; i++)
            for (V v : {triangles[i].a, triangles[i].b, triangles[i].c}) {
                n.low = minimum(n.low, v);
                n.high = maximum(n.high, v);
            }
        int index = int(nodes.size());
        nodes.push_back(n);
        if (end - begin > 12) {
            V extent = n.high - n.low;
            int axis = extent.x > extent.y ? 0 : 1;
            if (extent.z > component(extent, axis))
                axis = 2;
            int mid = (begin + end) / 2;
            std::nth_element(triangles.begin() + begin, triangles.begin() + mid, triangles.begin() + end,
                             [axis](const Triangle &a, const Triangle &b) {
                                 return component(a.a + a.b + a.c, axis) < component(b.a + b.b + b.c, axis);
                             });
            nodes[index].left = build(begin, mid);
            nodes[index].right = build(mid, end);
        }
        return index;
    }
    static double boxDistance(const Node &n, V p) {
        double sum = 0;
        for (int a = 0; a < 3; a++) {
            double v = component(p, a);
            double d = std::max({component(n.low, a) - v, 0., v - component(n.high, a)});
            sum += d * d;
        }
        return sum;
    }
    void nearestNode(int index, V p, double &best) const {
        const Node &n = nodes[index];
        if (boxDistance(n, p) > best)
            return;
        if (n.left < 0) {
            for (int i = n.begin; i < n.end; i++) {
                V d = p - nearest(p, triangles[i]);
                best = std::min(best, dot(d, d));
            }
        } else {
            int first = n.left, second = n.right;
            if (boxDistance(nodes[first], p) > boxDistance(nodes[second], p))
                std::swap(first, second);
            nearestNode(first, p, best);
            nearestNode(second, p, best);
        }
    }
    int intersections(int index, V p, V direction) const {
        const auto &n = nodes[index];
        double low = 0, high = 1e9;
        for (int a = 0; a < 3; a++) {
            double t1 = (component(n.low, a) - component(p, a)) / component(direction, a);
            double t2 = (component(n.high, a) - component(p, a)) / component(direction, a);
            if (t1 > t2)
                std::swap(t1, t2);
            low = std::max(low, t1);
            high = std::min(high, t2);
        }
        if (low > high)
            return 0;
        if (n.left >= 0)
            return intersections(n.left, p, direction) + intersections(n.right, p, direction);
        int hits = 0;
        for (int i = n.begin; i < n.end; i++) {
            const auto &t = triangles[i];
            V ab = t.b - t.a, ac = t.c - t.a, h = cross(direction, ac);
            double det = dot(ab, h);
            if (std::abs(det) < 1e-12)
                continue;
            V s = p - t.a;
            double u = dot(s, h) / det;
            if (u < 0 || u > 1)
                continue;
            V q = cross(s, ab);
            double v = dot(direction, q) / det;
            if (v >= 0 && u + v <= 1 && dot(ac, q) / det > 1e-9)
                hits++;
        }
        return hits;
    }
    double distance(V p) const {
        double best = 1e9;
        nearestNode(0, p, best);
        bool inside = false;
        if (solid && boxDistance(nodes[0], p) == 0) {
            int votes = intersections(0, p, {.912311, .371812, .173519}) % 2 +
                        intersections(0, p, {-.251831, .933711, -.17517}) % 2 +
                        intersections(0, p, {.183731, -.77117, .311773}) % 2;
            inside = votes >= 2;
        }
        return std::sqrt(best) * (inside ? -1 : 1);
    }
    static std::vector<unsigned char> decode(const std::string &text) {
        const std::string alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        std::array<int, 256> lookup;
        lookup.fill(-1);
        for (int i = 0; i < 64; i++)
            lookup[static_cast<unsigned char>(alphabet[i])] = i;
        std::vector<unsigned char> result;
        unsigned int value = 0;
        int bits = 0;
        for (unsigned char c : text) {
            if (lookup[c] < 0)
                continue;
            value = (value << 6) | lookup[c];
            bits += 6;
            if (bits >= 8) {
                bits -= 8;
                result.push_back(static_cast<unsigned char>(value >> bits));
            }
        }
        return result;
    }
    template <class T> static T read(const std::vector<unsigned char> &b, size_t offset) {
        if (offset + sizeof(T) > b.size())
            throw std::runtime_error("Mesh buffer outside bounds");
        T value;
        std::memcpy(&value, b.data() + offset, sizeof(value));
        return value;
    }
    static PinMesh load(const std::filesystem::path &path, bool solid) {
        std::ifstream input(path);
        Json json;
        input >> json;
        const auto &blob = json["Data"]["RootChunk"]["renderResourceBlob"]["Data"];
        const auto &h = blob["header"];
        auto buffer = decode(blob["renderBuffer"]["Bytes"].get<std::string>());
        auto xyz = [](const Json &j) { return V{j["X"], j["Y"], j["Z"]}; };
        V offset = xyz(h["quantizationOffset"]), scale = xyz(h["quantizationScale"]);
        PinMesh mesh;
        mesh.solid = solid;
        for (const auto &chunk : h["renderChunkInfos"]) {
            if (chunk["lodMask"] != 1)
                continue;
            const auto &vb = chunk["chunkVertices"];
            const auto &ib = chunk["chunkIndices"];
            if (ib["pe"] != "IBCT_IndexUShort")
                throw std::runtime_error("Unsupported index format");
            size_t start = vb["byteOffsets"]["Elements"][0],
                   stride = vb["vertexLayout"]["slotStrides"]["Elements"][0];
            std::vector<V> vertices;
            for (int i = 0; i < chunk["numVertices"]; ++i) {
                size_t p = start + i * stride;
                vertices.push_back({offset.x + scale.x * read<int16_t>(buffer, p) / 32767.,
                                    offset.y + scale.y * read<int16_t>(buffer, p + 2) / 32767.,
                                    offset.z + scale.z * read<int16_t>(buffer, p + 4) / 32767.});
            }
            size_t index = h["indexBufferOffset"].get<size_t>() + ib["teOffset"].get<size_t>();
            for (int i = 0; i < chunk["numIndices"]; i += 3) {
                Triangle t{vertices.at(read<uint16_t>(buffer, index + i * 2)),
                           vertices.at(read<uint16_t>(buffer, index + (i + 1) * 2)),
                           vertices.at(read<uint16_t>(buffer, index + (i + 2) * 2))};
                V area = cross(t.b - t.a, t.c - t.a);
                if (dot(area, area) > 1e-20)
                    mesh.triangles.push_back(t);
            }
        }
        if (mesh.triangles.empty())
            throw std::runtime_error("Empty mesh");
        mesh.build(0, int(mesh.triangles.size()));
        return mesh;
    }
};

struct PinBody {
    std::vector<PinMesh> meshes;
    std::vector<float> field;
    V origin;
    std::array<int, 3> count{};
    static constexpr double step = .002;
    static std::string prefix(const std::string &kind) {
        const std::pair<const char *, const char *> names[] = {
            {"frag", "001__frag_grenade_01"},           {"ozob", "001__frag_grenade_02"},
            {"flash", "002__flash_grenade_01"},         {"incendiary", "004__incendiary_grenade_01"},
            {"biohazard", "005__biohazard_grenade_01"}, {"smoke", "010__smoke_grenade_01"}};
        std::string prefix;
        for (auto [name, mesh] : names)
            if (kind == name)
                prefix = "w_explosives_" + std::string(mesh);
        if (prefix.empty())
            throw std::runtime_error("Unknown prop");
        return prefix;
    }
    static PinBody load(const std::filesystem::path &dir, const std::string &kind) {
        const auto name = prefix(kind);
        PinBody body;
        body.meshes.push_back(PinMesh::load(dir / (name + ".mesh.json"), true));
        for (const char *suffix : {"_lug_01.mesh.json", "_diodes.mesh.json"})
            if (std::filesystem::exists(dir / (name + suffix)))
                body.meshes.push_back(PinMesh::load(dir / (name + suffix), false));
        return body;
    }
    static V pinAxisCentre(const std::filesystem::path &dir, const std::string &kind) {
        auto mesh = PinMesh::load(dir / (prefix(kind) + "_safe_pin_01.mesh.json"), false);
        V low{1e9, 1e9, 1e9}, high{-1e9, -1e9, -1e9};
        for (const auto &t : mesh.triangles)
            for (V v : {t.a, t.b, t.c}) {
                if (v.y > -.0165) {
                    low = PinMesh::minimum(low, v);
                    high = PinMesh::maximum(high, v);
                }
            }
        if (low.x > high.x)
            throw std::runtime_error("Pin shaft cross-section missing");
        return {(low.x + high.x) * .5, 0, (low.z + high.z) * .5};
    }
    double distanceExact(V p) const {
        double value = 1e9;
        for (const auto &mesh : meshes)
            value = std::min(value, mesh.distance(p));
        return value;
    }
    void buildField() {
        V low{1e9, 1e9, 1e9}, high{-1e9, -1e9, -1e9};
        for (const auto &mesh : meshes) {
            low = PinMesh::minimum(low, mesh.nodes[0].low);
            high = PinMesh::maximum(high, mesh.nodes[0].high);
        }
        origin = low - V{.025, .025, .025};
        high = high + V{.025, .025, .025};
        for (int i = 0; i < 3; i++)
            count[i] =
                int(std::ceil((PinMesh::component(high, i) - PinMesh::component(origin, i)) / step)) + 1;
        field.resize(size_t(count[0]) * count[1] * count[2]);
        for (int z = 0; z < count[2]; z++)
            for (int y = 0; y < count[1]; y++)
                for (int x = 0; x < count[0]; x++)
                    field[x + count[0] * (y + count[1] * z)] =
                        float(distanceExact(origin + V{x * step, y * step, z * step}));
    }
    double distance(V p) const {
        if (field.empty())
            return distanceExact(p);
        V cell = (p - origin) * (1 / step);
        int c[3];
        double f[3];
        for (int i = 0; i < 3; i++) {
            double v = PinMesh::component(cell, i);
            c[i] = int(std::floor(v));
            f[i] = v - c[i];
            if (c[i] < 0 || c[i] >= count[i] - 1)
                return .024;
        }
        double result = 0;
        for (int z = 0; z < 2; z++)
            for (int y = 0; y < 2; y++)
                for (int x = 0; x < 2; x++)
                    result += field[c[0] + x + count[0] * (c[1] + y + count[1] * (c[2] + z))] *
                              (x ? f[0] : 1 - f[0]) * (y ? f[1] : 1 - f[1]) * (z ? f[2] : 1 - f[2]);
        // Used during optimization. The final trajectory is checked against
        // the original triangles rather than this sampled field.
        return result;
    }
    double clearance(T prop, T meta, T proximal, T distal, double handedness, bool exact = false) const {
        auto local = [&](V p) { return rot(inv(prop.q), p - prop.p); };
        V a = local(meta.p), b = local(proximal.p), c = local(distal.p);
        double length = std::sqrt(dot(distal.p - proximal.p, distal.p - proximal.p)) * .75;
        V tip = local(distal.p + rot(distal.q, {handedness * length, 0, 0}));
        double result = 1e9;
        const std::array<V, 4> points = {a, b, c, tip};
        // Conservative envelope of GraspPose.cpp's measured first-person
        // thumb, including its pad offset and the taper at the fingertip.
        const double radii[] = {.012, .0134, .0102};
        for (int segment = 0; segment < 3; ++segment) {
            int steps = std::max(2, int(std::ceil(std::sqrt(dot(points[segment + 1] - points[segment],
                                                                points[segment + 1] - points[segment])) /
                                                  .004)));
            // The first quarter of the metacarpal is inside the palm.
            for (int i = segment == 0 ? (steps + 3) / 4 : 0; i <= steps; ++i) {
                V p = points[segment] + (points[segment + 1] - points[segment]) * (double(i) / steps);
                double radius = radii[segment];
                if (segment == 2) {
                    constexpr double sections[][2] = {{0, .0102},   {.25, .0102}, {.45, .0089}, {.65, .0085},
                                                      {.75, .0082}, {.85, .0076}, {.95, .0062}, {1, .0045}};
                    double u = double(i) / steps;
                    for (int j = 1; j < 8; j++)
                        if (u <= sections[j][0]) {
                            double t = (u - sections[j - 1][0]) / (sections[j][0] - sections[j - 1][0]);
                            radius = sections[j - 1][1] + (sections[j][1] - sections[j - 1][1]) * t;
                            break;
                        }
                }
                result = std::min(result, (exact ? distanceExact(p) : distance(p)) - radius);
            }
        }
        return result;
    }
};
