// mirror_anims: a player weapon animation set mirrored onto the other hand -- the left-handed set of a weapon,
// generated from the game's own right-handed one (tools/lefthanded_anims/generate.py runs it).
//
//   mirror_anims <in.glb> <out.glb> [suffix]        one set
//   mirror_anims --list <jobs.txt>                  many: each line "in.glb|out.glb"
//
// IN is a WolvenKit export of a player .anims with its additive animations left as deltas (AnimTool export, schema
// wkit.cp2077.gltf.anims; NOT cp77tools export, which bakes them onto the rest pose). OUT holds every animation of
// IN mirrored across the body's midplane under its own name (plus the suffix, if one is given): the single-wield
// animations the ranged graph plays (idlepose_shoulder, add_recoil_single_shot_shoulder, equip, first_equip,
// reload ...), now of the LEFT hand -- played while the weapon is alone there (src/Hooks/LeftHandedAnimation.cpp).
//
// THE MIRROR IS THE RIG'S, read from its rest pose rather than assumed (the rule src/Anim/LeftHanded.cpp follows at
// runtime). Every bone has a twin -- Left* <-> Right*, left <-> right in the control joints, a centre bone is its
// own -- and in model space a twin is the bone reflected across the midplane, up to a reflection of its own axes:
//
//     X_twin = M * X_bone * S_bone       M = the midplane reflection,  S_bone orthogonal, det -1
//
// S is solved per bone from the rest pose, exactly: for the hands and fingers it is a signed permutation, for the
// odd control joint of this rig (arms_gun_space, tilted 5 degrees off any mirror) it is whatever reflection maps the
// rest pose onto itself. A local transform, parent p, then mirrors as
//
//     rotation   L_twin = S_p^T * L_bone * S_bone        translation   t_twin = S_p^T * t_bone
//
// An additive animation is a delta the game applies after the pose it is added to -- local = base * d,
// t = base_t + d_t (read off WolvenKit's baked export: it is rest * d to the bit) -- so its rotations mirror by the
// bone's own reflection, d_twin = S_bone^T * d * S_bone, and its translations like any other. A bone whose rest
// POSITION is no mirror of its twin's keeps its own translation: the spine's up-vector targets sit 40 cm to one side
// of the midplane, and mirrored they would turn the spine's up the other way. Float tracks are swapped by name
// (leftHandWorldSpace <-> rightHandWorldSpace ...), animation events are kept.
//
// FALLBACK FRAMES, written beside OUT as OUT.fallback.json. A set's animation data streams in when the set is first
// asked for, and until it is in the engine poses the character from the set's fallback frames -- or, with none, from
// the rig's reference pose (measured: 13-27 ms of it on the first draw of each weapon). Every animation the game's own
// set gives fallback frames to gets them here too, from the mirrored animation: its first frame, and its last when
// the game's had more than one. The game's interior frames (a reload's twelve) match no moment of their animation, so
// they are not imitated; the window they cover only exists until the set's one data chunk is in. The format, read
// off Cyberpunk2077.exe 2.5 (the set's fallback setup sub_14053383C, pools sub_14053336C, frame sub_140533A48) and
// checked to quantization on the game's Lexington set: frames back to back, each
//     u16 positionValue[P], u16 positionBone[P], u16 rotationValue[R], u16 rotationBone[R]   (bones ascending)
// sized by fallbackDataAddresses, then the value pools: a position in a u32 (x bits 22-31, y 12-21, z 0-9, each
// v = u / 511.5 - 1, metres, so +-1 m), a rotation in four u16 stored w, z, y, x (v = u / 32767.5 - 1), both in the
// engine's Z-up axes. A bone the frame leaves out stays at the rig's rest pose.
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <map>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

#include "vendor/nlohmann/json.hpp"

using json = nlohmann::json;

namespace {

using M3 = std::array<std::array<double, 3>, 3>;

M3 Identity() { return {{{1, 0, 0}, {0, 1, 0}, {0, 0, 1}}}; }

M3 Mul(const M3& a, const M3& b) {
    M3 o{};
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j) o[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
    return o;
}

M3 T(const M3& a) {
    M3 o{};
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j) o[i][j] = a[j][i];
    return o;
}

std::array<double, 3> Apply(const M3& a, const std::array<double, 3>& v) {
    return {a[0][0] * v[0] + a[0][1] * v[1] + a[0][2] * v[2], a[1][0] * v[0] + a[1][1] * v[1] + a[1][2] * v[2],
            a[2][0] * v[0] + a[2][1] * v[1] + a[2][2] * v[2]};
}

M3 FromQuat(double x, double y, double z, double w) {
    return {{{1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)},
             {2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)},
             {2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)}}};
}

std::array<double, 4> ToQuat(const M3& m) {
    std::array<double, 4> q{};
    const double t = m[0][0] + m[1][1] + m[2][2];
    if (t > m[0][0] && t > m[1][1] && t > m[2][2]) {
        const double s = std::sqrt(std::max(t + 1.0, 1e-12)) * 2;
        q = {(m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s, 0.25 * s};
    } else if (m[0][0] >= m[1][1] && m[0][0] >= m[2][2]) {
        const double s = std::sqrt(std::max(1.0 + m[0][0] - m[1][1] - m[2][2], 1e-12)) * 2;
        q = {0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s, (m[2][1] - m[1][2]) / s};
    } else if (m[1][1] >= m[2][2]) {
        const double s = std::sqrt(std::max(1.0 + m[1][1] - m[0][0] - m[2][2], 1e-12)) * 2;
        q = {(m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s, (m[0][2] - m[2][0]) / s};
    } else {
        const double s = std::sqrt(std::max(1.0 + m[2][2] - m[0][0] - m[1][1], 1e-12)) * 2;
        q = {(m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s, (m[1][0] - m[0][1]) / s};
    }
    const double n = std::sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
    for (auto& c : q) c /= n;
    if (q[3] < 0)
        for (auto& c : q) c = -c;
    return q;
}

// The signed permutation nearest to m, and by how much m misses it; false when no permutation is near.
bool Snap(const M3& m, M3& s, double& miss) {
    s = {};
    bool used[3] = {};
    for (int i = 0; i < 3; ++i) {
        int j = 0;
        for (int k = 1; k < 3; ++k)
            if (std::fabs(m[i][k]) > std::fabs(m[i][j])) j = k;
        if (used[j]) return false;
        used[j] = true;
        s[i][j] = m[i][j] < 0 ? -1.0 : 1.0;
    }
    miss = 0;
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j) miss = std::max(miss, std::fabs(m[i][j] - s[i][j]));
    return true;
}

// Left* <-> Right*, left <-> right -- the twin's name.
std::string SwapSide(const std::string& name) {
    std::string out;
    for (size_t i = 0; i < name.size();) {
        auto at = [&](const char* w) { return name.compare(i, std::strlen(w), w) == 0; };
        if (at("Left")) { out += "Right"; i += 4; }
        else if (at("Right")) { out += "Left"; i += 5; }
        else if (at("left")) { out += "right"; i += 4; }
        else if (at("right")) { out += "left"; i += 5; }
        else out += name[i++];
    }
    return out;
}

struct Glb {
    json doc;
    std::vector<uint8_t> blob;
};

Glb ReadGlb(const std::string& path) {
    std::ifstream f(path, std::ios::binary);
    if (!f) throw std::runtime_error(path + ": cannot open");
    std::vector<uint8_t> data((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
    if (data.size() < 28 || std::memcmp(data.data(), "glTF", 4) != 0) throw std::runtime_error(path + ": not a .glb");
    uint32_t jlen, blen;
    std::memcpy(&jlen, data.data() + 12, 4);
    Glb g;
    g.doc = json::parse(data.begin() + 20, data.begin() + 20 + jlen);
    if (data.size() >= 28 + jlen) {
        std::memcpy(&blen, data.data() + 20 + jlen, 4);
        g.blob.assign(data.begin() + 28 + jlen, data.begin() + 28 + jlen + blen);
    }
    return g;
}

int Width(const std::string& type) {
    if (type == "SCALAR") return 1;
    if (type == "VEC2") return 2;
    if (type == "VEC3") return 3;
    if (type == "VEC4") return 4;
    if (type == "MAT4") return 16;
    throw std::runtime_error("accessor type " + type);
}

std::vector<float> Accessor(const Glb& g, int index, int& width) {
    const json& acc = g.doc["accessors"][index];
    const json& view = g.doc["bufferViews"][acc["bufferView"].get<int>()];
    if (acc["componentType"].get<int>() != 5126) throw std::runtime_error("non-float accessor");
    width = Width(acc["type"]);
    const size_t start = view.value("byteOffset", 0) + acc.value("byteOffset", 0);
    const size_t count = acc["count"].get<size_t>() * width;
    if (view.contains("byteStride") && view["byteStride"].get<int>() != width * 4)
        throw std::runtime_error("strided accessor");
    std::vector<float> out(count);
    std::memcpy(out.data(), g.blob.data() + start, count * 4);
    return out;
}

struct Packer {
    std::vector<uint8_t> blob;
    json views = json::array();
    json accessors = json::array();

    int Add(const std::vector<float>& values, int width, const char* type, bool minmax) {
        while (blob.size() % 4) blob.push_back(0);
        views.push_back({{"buffer", 0}, {"byteOffset", blob.size()}, {"byteLength", values.size() * 4}});
        const size_t at = blob.size();
        blob.resize(at + values.size() * 4);
        std::memcpy(blob.data() + at, values.data(), values.size() * 4);
        json acc = {{"bufferView", views.size() - 1}, {"componentType", 5126}, {"count", values.size() / width},
                    {"type", type}};
        if (minmax && !values.empty()) {
            std::vector<float> lo(width, values[0]), hi(width, values[0]);
            for (size_t i = 0; i < values.size(); ++i) {
                lo[i % width] = std::min(lo[i % width], values[i]);
                hi[i % width] = std::max(hi[i % width], values[i]);
            }
            acc["min"] = lo;
            acc["max"] = hi;
        }
        accessors.push_back(acc);
        return static_cast<int>(accessors.size() - 1);
    }
};

void WriteGlb(const std::string& path, const json& doc, std::vector<uint8_t> blob) {
    std::string body = doc.dump();
    while (body.size() % 4) body += ' ';
    while (blob.size() % 4) blob.push_back(0);
    const uint32_t total = static_cast<uint32_t>(12 + 8 + body.size() + 8 + blob.size());
    std::ofstream f(path, std::ios::binary);
    if (!f) throw std::runtime_error(path + ": cannot write");
    auto u32 = [&](uint32_t v) { f.write(reinterpret_cast<const char*>(&v), 4); };
    f.write("glTF", 4); u32(2); u32(total);
    u32(static_cast<uint32_t>(body.size())); f.write("JSON", 4); f.write(body.data(), body.size());
    u32(static_cast<uint32_t>(blob.size())); f.write("BIN\0", 4);
    f.write(reinterpret_cast<const char*>(blob.data()), blob.size());
}

// The skeleton of an export: hierarchy, rest pose, twins and their reflections.
struct Rig {
    std::vector<std::string> names;
    std::vector<int> parent, twin;
    std::vector<M3> S;
    std::vector<bool> keepTranslation;
    M3 M = Identity();
    double residual = 0;

    explicit Rig(const json& doc) {
        const json& nodes = doc["nodes"];
        const size_t n = nodes.size();
        names.resize(n);
        parent.assign(n, -1);
        twin.assign(n, -1);
        std::map<std::string, int> index;
        for (size_t i = 0; i < n; ++i) {
            names[i] = nodes[i].value("name", "");
            index[names[i]] = static_cast<int>(i);
            for (const auto& c : nodes[i].value("children", json::array())) parent[c.get<int>()] = static_cast<int>(i);
        }
        for (size_t i = 0; i < n; ++i) {
            auto it = index.find(SwapSide(names[i]));
            if (it == index.end()) throw std::runtime_error("bone " + names[i] + " has no twin");
            twin[i] = it->second;
        }
        std::vector<M3> localR(n), modelR(n);
        std::vector<std::array<double, 3>> localT(n), modelT(n);
        for (size_t i = 0; i < n; ++i) {
            auto r = nodes[i].value("rotation", std::vector<double>{0, 0, 0, 1});
            auto t = nodes[i].value("translation", std::vector<double>{0, 0, 0});
            localR[i] = FromQuat(r[0], r[1], r[2], r[3]);
            localT[i] = {t[0], t[1], t[2]};
        }
        std::vector<bool> done(n, false);
        std::function<void(int)> resolve = [&](int i) {
            if (done[i]) return;
            const int p = parent[i];
            if (p < 0) { modelR[i] = localR[i]; modelT[i] = localT[i]; }
            else {
                resolve(p);
                modelR[i] = Mul(modelR[p], localR[i]);
                auto d = Apply(modelR[p], localT[i]);
                modelT[i] = {d[0] + modelT[p][0], d[1] + modelT[p][1], d[2] + modelT[p][2]};
            }
            done[i] = true;
        };
        for (size_t i = 0; i < n; ++i) resolve(static_cast<int>(i));
        // the midplane: the axis the hands' rest positions disagree on
        const int l = index.at("LeftHand"), r = index.at("RightHand");
        int axis = 0;
        for (int k = 1; k < 3; ++k)
            if (std::fabs(modelT[l][k] - modelT[r][k]) > std::fabs(modelT[l][axis] - modelT[r][axis])) axis = k;
        M[axis][axis] = -1;
        S.resize(n);
        keepTranslation.assign(n, false);
        for (size_t i = 0; i < n; ++i) {
            const M3 s = Mul(Mul(T(modelR[i]), M), modelR[twin[i]]);
            M3 snapped;
            double miss = 1;
            const bool near = Snap(s, snapped, miss);
            // a twin pair is a mirror up to a signed permutation of the axes; only a centre bone may be off it
            if (twin[i] != static_cast<int>(i) && (!near || miss > 0.01))
                throw std::runtime_error(names[i] + ": rest pose is no mirror of " + names[twin[i]]);
            S[i] = near && miss < 1e-4 ? snapped : s;
            residual = std::max(residual, miss);
            const auto mt = Apply(M, modelT[i]);
            double pm = 0;
            for (int k = 0; k < 3; ++k) pm = std::max(pm, std::fabs(mt[k] - modelT[twin[i]][k]));
            keepTranslation[i] = pm > 0.001;
        }
        for (size_t i = 0; i < n; ++i) {
            const int p = parent[i], pt = parent[twin[i]];
            if ((p < 0) != (pt < 0) || (p >= 0 && twin[p] != pt))
                throw std::runtime_error(names[i] + " and its twin hang from parents that are not twins");
        }
    }

    const M3& ParentS(int i) const { return parent[i] < 0 ? M : S[parent[i]]; }
};

using Q4 = std::array<double, 4>;
using V3 = std::array<double, 3>;

// A pose of every bone, in the skin's bone order: local rotations (glTF x y z w) and translations.
struct Pose {
    std::vector<Q4> rot;
    std::vector<V3> pos;
};

// The fallback frames of one set, encoded as the engine reads them (see the header).
struct Fallback {
    std::vector<uint8_t> frames;
    std::vector<uint32_t> positions;
    std::vector<uint64_t> rotations;
    std::map<uint32_t, int> positionIndex;
    std::map<uint64_t, int> rotationIndex;
    std::vector<std::array<int, 3>> descs;   // positions, rotations, float tracks
    std::vector<int> addresses;             // a frame's size in bytes
    std::vector<int> descIndex, addressIndex;

    static uint32_t Quantize(double v, double scale, uint32_t top) {
        return static_cast<uint32_t>(std::clamp(std::llround((v + 1.0) * scale), 0LL, static_cast<long long>(top)));
    }

    template <typename K>
    static int Pooled(std::vector<K>& pool, std::map<K, int>& index, K value) {
        const auto it = index.find(value);
        if (it != index.end()) return it->second;
        if (pool.size() >= 0xFFFF) throw std::runtime_error("fallback value pool full");
        pool.push_back(value);
        return index[value] = static_cast<int>(pool.size() - 1);
    }

    // One frame: the bones the pose moves off the rest pose (by more than half a quantization step). Its index.
    int Add(const Pose& pose, const Pose& rest) {
        std::vector<uint16_t> posValue, posBone, rotValue, rotBone;
        for (size_t b = 0; b < pose.pos.size(); ++b) {
            double off = 0;
            for (int c = 0; c < 3; ++c) off = std::max(off, std::fabs(pose.pos[b][c] - rest.pos[b][c]));
            if (off <= 0.5 / 511.5) continue;
            // glTF (x, y, z) is the engine's (x, -z, y)
            const V3& p = pose.pos[b];
            const uint32_t packed = Quantize(p[0], 511.5, 1023) << 22 | Quantize(-p[2], 511.5, 1023) << 12 |
                                    Quantize(p[1], 511.5, 1023);
            posValue.push_back(static_cast<uint16_t>(Pooled(positions, positionIndex, packed)));
            posBone.push_back(static_cast<uint16_t>(b));
        }
        for (size_t b = 0; b < pose.rot.size(); ++b) {
            Q4 q = pose.rot[b];
            const double n = std::sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
            for (auto& c : q) c /= n;
            const Q4& r = rest.rot[b];
            const double dot = std::fabs(q[0] * r[0] + q[1] * r[1] + q[2] * r[2] + q[3] * r[3]);
            if (dot >= std::cos(0.025 * 3.14159265358979 / 180.0)) continue;   // within 0.05 degrees
            if (q[3] < 0)
                for (auto& c : q) c = -c;
            // the engine's (x, y, z, w) = glTF (x, -z, y, w), stored w, z, y, x
            const uint64_t packed = static_cast<uint64_t>(Quantize(q[3], 32767.5, 65535)) |
                                    static_cast<uint64_t>(Quantize(q[1], 32767.5, 65535)) << 16 |
                                    static_cast<uint64_t>(Quantize(-q[2], 32767.5, 65535)) << 32 |
                                    static_cast<uint64_t>(Quantize(q[0], 32767.5, 65535)) << 48;
            rotValue.push_back(static_cast<uint16_t>(Pooled(rotations, rotationIndex, packed)));
            rotBone.push_back(static_cast<uint16_t>(b));
        }
        auto put = [&](const std::vector<uint16_t>& v) {
            for (uint16_t x : v) { frames.push_back(static_cast<uint8_t>(x)); frames.push_back(static_cast<uint8_t>(x >> 8)); }
        };
        put(posValue); put(posBone); put(rotValue); put(rotBone);
        const std::array<int, 3> desc = {static_cast<int>(posBone.size()), static_cast<int>(rotBone.size()), 0};
        const int size = 4 * (desc[0] + desc[1]);
        auto slot = [](auto& table, const auto& value) {
            const auto it = std::find(table.begin(), table.end(), value);
            if (it != table.end()) return static_cast<int>(it - table.begin());
            if (table.size() >= 256) throw std::runtime_error("more than 256 fallback frame layouts");
            table.push_back(value);
            return static_cast<int>(table.size() - 1);
        };
        descIndex.push_back(slot(descs, desc));
        addressIndex.push_back(slot(addresses, size));
        return static_cast<int>(descIndex.size() - 1);
    }

    void Write(const std::string& path) const {
        std::string hex;
        auto bytes = [&](uint64_t v, int n) {
            static const char* digits = "0123456789abcdef";
            for (int i = 0; i < n; ++i, v >>= 8) { hex += digits[(v >> 4) & 15]; hex += digits[v & 15]; }
        };
        for (uint8_t b : frames) bytes(b, 1);
        for (uint32_t v : positions) bytes(v, 4);
        for (uint64_t v : rotations) bytes(v, 8);
        const json out = {{"buffer", hex}, {"frameDescs", descs}, {"dataAddresses", addresses},
                          {"descIndexes", descIndex}, {"addressIndexes", addressIndex},
                          {"numPositions", positions.size()}, {"numRotations", rotations.size()},
                          {"numFloatTracks", 0}};
        std::ofstream f(path, std::ios::binary);
        if (!f) throw std::runtime_error(path + ": cannot write");
        f << out.dump();
    }
};

void Mirror(const std::string& in, const std::string& out, const std::string& suffix) {
    const Glb g = ReadGlb(in);
    if (g.doc.value("animations", json::array()).empty()) {
        // rig bindings and tags only: nothing for the left hand, no set written
        std::printf("%s: no animations\n", in.c_str());
        return;
    }
    const Rig rig(g.doc);
    std::vector<int> trackTwin;
    const json& skin0 = g.doc["skins"][0];
    if (skin0.contains("extras") && skin0["extras"].contains("trackNames")) {
        std::vector<std::string> tracks = skin0["extras"]["trackNames"];
        for (size_t k = 0; k < tracks.size(); ++k) {
            const auto it = std::find(tracks.begin(), tracks.end(), SwapSide(tracks[k]));
            trackTwin.push_back(it == tracks.end() ? static_cast<int>(k) : static_cast<int>(it - tracks.begin()));
        }
    }
    Packer pack;
    json skin = skin0;
    if (skin.contains("inverseBindMatrices")) {
        int w;
        auto ibm = Accessor(g, skin["inverseBindMatrices"].get<int>(), w);
        skin["inverseBindMatrices"] = pack.Add(ibm, w, "MAT4", false);
    }
    // the bones as the set indexes them: the skin's joints, in order, and their rest pose
    std::vector<int> bone(g.doc["nodes"].size(), -1);
    Pose rest;
    for (const json& j : skin0["joints"]) {
        const int node = j.get<int>();
        bone[node] = static_cast<int>(rest.rot.size());
        const json& nd = g.doc["nodes"][node];
        const auto r = nd.value("rotation", std::vector<double>{0, 0, 0, 1});
        const auto t = nd.value("translation", std::vector<double>{0, 0, 0});
        rest.rot.push_back({r[0], r[1], r[2], r[3]});
        rest.pos.push_back({t[0], t[1], t[2]});
    }
    Fallback fallback;
    json animations = json::array();
    for (const json& anim : g.doc.value("animations", json::array())) {
        json extras = anim.value("extras", json::object());
        const bool additive = extras.value("animationType", std::string("Normal")) != "Normal";
        const size_t wanted = additive ? 0 : extras.value("fallbackFrameIndices", json::array()).size();
        Pose first = rest, last = rest;
        json samplers = json::array(), channels = json::array();
        for (const json& ch : anim["channels"]) {
            const json& sampler = anim["samplers"][ch["sampler"].get<int>()];
            const int node = ch["target"]["node"].get<int>();
            const std::string path = ch["target"]["path"];
            int tw, vw;
            const auto times = Accessor(g, sampler["input"].get<int>(), tw);
            auto values = Accessor(g, sampler["output"].get<int>(), vw);
            const M3& sp = rig.ParentS(node);
            const char* kind = "VEC3";
            if (path == "translation") {
                if (!rig.keepTranslation[node])
                    for (size_t k = 0; k + 2 < values.size(); k += 3) {
                        const auto v = Apply(T(sp), {values[k], values[k + 1], values[k + 2]});
                        for (int c = 0; c < 3; ++c) values[k + c] = static_cast<float>(v[c]);
                    }
            } else if (path == "rotation") {
                kind = "VEC4";
                const M3 left = additive ? T(rig.S[node]) : T(sp);
                for (size_t k = 0; k + 3 < values.size(); k += 4) {
                    const M3 r = Mul(Mul(left, FromQuat(values[k], values[k + 1], values[k + 2], values[k + 3])),
                                     rig.S[node]);
                    const auto q = ToQuat(r);
                    for (int c = 0; c < 4; ++c) values[k + c] = static_cast<float>(q[c]);
                }
            } else if (path == "scale") {
                for (size_t k = 0; k + 2 < values.size(); k += 3) {
                    const auto v = Apply(T(rig.S[node]), {values[k], values[k + 1], values[k + 2]});
                    for (int c = 0; c < 3; ++c) values[k + c] = static_cast<float>(std::fabs(v[c]));
                }
            } else {
                throw std::runtime_error("unexpected channel " + path);
            }
            samplers.push_back({{"input", pack.Add(times, 1, "SCALAR", true)},
                                {"interpolation", sampler.value("interpolation", std::string("LINEAR"))},
                                {"output", pack.Add(values, vw, kind, false)}});
            channels.push_back({{"sampler", samplers.size() - 1}, {"target", {{"node", rig.twin[node]}, {"path", path}}}});
            const int b = bone[rig.twin[node]];
            if (wanted && b >= 0 && values.size() >= static_cast<size_t>(vw) && path != "scale") {
                const size_t end = values.size() - vw;
                if (path == "rotation") {
                    first.rot[b] = {values[0], values[1], values[2], values[3]};
                    last.rot[b] = {values[end], values[end + 1], values[end + 2], values[end + 3]};
                } else {
                    first.pos[b] = {values[0], values[1], values[2]};
                    last.pos[b] = {values[end], values[end + 1], values[end + 2]};
                }
            }
        }
        if (wanted) {
            json frames = json::array({fallback.Add(first, rest)});
            if (wanted > 1) frames.push_back(fallback.Add(last, rest));
            extras["fallbackFrameIndices"] = frames;
        } else {
            extras["fallbackFrameIndices"] = json::array();
        }
        for (const char* key : {"constTrackKeys", "trackKeys"}) {
            if (!extras.contains(key) || !extras[key].is_array()) continue;
            for (json& k : extras[key]) {
                const int t = k["trackIndex"].get<int>();
                if (t >= 0 && t < static_cast<int>(trackTwin.size())) k["trackIndex"] = trackTwin[t];
            }
        }
        animations.push_back({{"name", anim.value("name", std::string()) + suffix}, {"channels", channels},
                              {"samplers", samplers}, {"extras", extras}});
    }
    json doc = g.doc;
    doc.erase("accessors");
    doc.erase("bufferViews");
    doc["skins"][0] = skin;
    doc["animations"] = animations;
    doc["accessors"] = pack.accessors;
    doc["bufferViews"] = pack.views;
    doc["buffers"] = json::array({{{"byteLength", pack.blob.size()}}});
    WriteGlb(out, doc, pack.blob);
    fallback.Write(out + ".fallback.json");
    std::printf("%s: %zu animations mirrored (largest centre-bone tilt off a mirror %.4f), %zu fallback frames\n",
                out.c_str(), animations.size(), rig.residual, fallback.descIndex.size());
}

}  // namespace

int main(int argc, char** argv) {
    try {
        if (argc == 3 && std::string(argv[1]) == "--list") {
            std::ifstream list(argv[2]);
            std::string line;
            int failures = 0;
            while (std::getline(list, line)) {
                if (!line.empty() && line.back() == '\r') line.pop_back();
                const auto bar = line.find('|');
                if (bar == std::string::npos) continue;
                try {
                    Mirror(line.substr(0, bar), line.substr(bar + 1), "");
                } catch (const std::exception& e) {
                    std::fprintf(stderr, "%s: %s\n", line.substr(0, bar).c_str(), e.what());
                    ++failures;
                }
            }
            return failures ? 1 : 0;
        }
        if (argc < 3 || argc > 4) {
            std::fprintf(stderr, "usage: mirror_anims <in.glb> <out.glb> [suffix]\n       mirror_anims --list <jobs.txt>\n");
            return 2;
        }
        Mirror(argv[1], argv[2], argc == 4 ? argv[3] : "");
        return 0;
    } catch (const std::exception& e) {
        std::fprintf(stderr, "%s\n", e.what());
        return 1;
    }
}
