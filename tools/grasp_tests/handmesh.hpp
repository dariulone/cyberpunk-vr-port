// The game's first-person hand (handmesh.py's mesh/hand_r.bin, hand_l.bin: a0_000_pwa_fpp__full_r/_l), skinned to the
// solved fingers and drawn in place of the solver's capsules; and the real skin's clearance to the thing per finger.
// Included by harness.cpp after GraspPose.cpp (Pose, Rot, InFrame, Sdf...).
//
// Frames: the mesh's bind and vertices are taken into its hand joint's bind frame (B_hand^-1); a joint's skinning matrix
// is the rig bone's solved pose in the hand bone's frame times the joint's bind in the hand's bind frame, inverted. The
// mesh's muscle joints (r_ind_knuckle_*_mscl_JNT...) are not in the anim rig: each follows two rig bones (the metacarpal
// and the phalanx across the knuckle, say) half way, as the game's deformation rig drives them between. Joints off the
// hand (the wrist, the forearm) stay with the hand; triangles mostly off the hand are not drawn.
#pragma once
#include <array>
#include <map>

namespace hm {

using M4 = std::array<float, 16>;   // column-major, as glTF

static M4 Mul(const M4& a, const M4& b) {
    M4 o{};
    for (int c = 0; c < 4; ++c)
        for (int r = 0; r < 4; ++r) {
            float s = 0.0f;
            for (int k = 0; k < 4; ++k) s += a[k * 4 + r] * b[c * 4 + k];
            o[c * 4 + r] = s;
        }
    return o;
}

static bool Inverse(const M4& m, M4& out) {
    const float* a = m.data();
    float inv[16];
    inv[0] = a[5] * a[10] * a[15] - a[5] * a[11] * a[14] - a[9] * a[6] * a[15] + a[9] * a[7] * a[14] + a[13] * a[6] * a[11] - a[13] * a[7] * a[10];
    inv[4] = -a[4] * a[10] * a[15] + a[4] * a[11] * a[14] + a[8] * a[6] * a[15] - a[8] * a[7] * a[14] - a[12] * a[6] * a[11] + a[12] * a[7] * a[10];
    inv[8] = a[4] * a[9] * a[15] - a[4] * a[11] * a[13] - a[8] * a[5] * a[15] + a[8] * a[7] * a[13] + a[12] * a[5] * a[11] - a[12] * a[7] * a[9];
    inv[12] = -a[4] * a[9] * a[14] + a[4] * a[10] * a[13] + a[8] * a[5] * a[14] - a[8] * a[6] * a[13] - a[12] * a[5] * a[10] + a[12] * a[6] * a[9];
    inv[1] = -a[1] * a[10] * a[15] + a[1] * a[11] * a[14] + a[9] * a[2] * a[15] - a[9] * a[3] * a[14] - a[13] * a[2] * a[11] + a[13] * a[3] * a[10];
    inv[5] = a[0] * a[10] * a[15] - a[0] * a[11] * a[14] - a[8] * a[2] * a[15] + a[8] * a[3] * a[14] + a[12] * a[2] * a[11] - a[12] * a[3] * a[10];
    inv[9] = -a[0] * a[9] * a[15] + a[0] * a[11] * a[13] + a[8] * a[1] * a[15] - a[8] * a[3] * a[13] - a[12] * a[1] * a[11] + a[12] * a[3] * a[9];
    inv[13] = a[0] * a[9] * a[14] - a[0] * a[10] * a[13] - a[8] * a[1] * a[14] + a[8] * a[2] * a[13] + a[12] * a[1] * a[10] - a[12] * a[2] * a[9];
    inv[2] = a[1] * a[6] * a[15] - a[1] * a[7] * a[14] - a[5] * a[2] * a[15] + a[5] * a[3] * a[14] + a[13] * a[2] * a[7] - a[13] * a[3] * a[6];
    inv[6] = -a[0] * a[6] * a[15] + a[0] * a[7] * a[14] + a[4] * a[2] * a[15] - a[4] * a[3] * a[14] - a[12] * a[2] * a[7] + a[12] * a[3] * a[6];
    inv[10] = a[0] * a[5] * a[15] - a[0] * a[7] * a[13] - a[4] * a[1] * a[15] + a[4] * a[3] * a[13] + a[12] * a[1] * a[7] - a[12] * a[3] * a[5];
    inv[14] = -a[0] * a[5] * a[14] + a[0] * a[6] * a[13] + a[4] * a[1] * a[14] - a[4] * a[2] * a[13] - a[12] * a[1] * a[6] + a[12] * a[2] * a[5];
    inv[3] = -a[1] * a[6] * a[11] + a[1] * a[7] * a[10] + a[5] * a[2] * a[11] - a[5] * a[3] * a[10] - a[9] * a[2] * a[7] + a[9] * a[3] * a[6];
    inv[7] = a[0] * a[6] * a[11] - a[0] * a[7] * a[10] - a[4] * a[2] * a[11] + a[4] * a[3] * a[10] + a[8] * a[2] * a[7] - a[8] * a[3] * a[6];
    inv[11] = -a[0] * a[5] * a[11] + a[0] * a[7] * a[9] + a[4] * a[1] * a[11] - a[4] * a[3] * a[9] - a[8] * a[1] * a[7] + a[8] * a[3] * a[5];
    inv[15] = a[0] * a[5] * a[10] - a[0] * a[6] * a[9] - a[4] * a[1] * a[10] + a[4] * a[2] * a[9] + a[8] * a[1] * a[6] - a[8] * a[2] * a[5];
    const float det = a[0] * inv[0] + a[1] * inv[4] + a[2] * inv[8] + a[3] * inv[12];
    if (std::fabs(det) < 1e-12f) return false;
    for (int i = 0; i < 16; ++i) out[i] = inv[i] / det;
    return true;
}

static M4 FromPose(const Pose& p) {
    const float ex[3] = {1, 0, 0}, ey[3] = {0, 1, 0}, ez[3] = {0, 0, 1};
    float c0[3], c1[3], c2[3];
    Rot(p.q, ex, c0);
    Rot(p.q, ey, c1);
    Rot(p.q, ez, c2);
    return {c0[0], c0[1], c0[2], 0, c1[0], c1[1], c1[2], 0, c2[0], c2[1], c2[2], 0, p.p[0], p.p[1], p.p[2], 1};
}

static void Point(const M4& m, const float* p, float* o) {
    for (int r = 0; r < 3; ++r) o[r] = m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r];
}
static void Dir(const M4& m, const float* p, float* o) {
    for (int r = 0; r < 3; ++r) o[r] = m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2];
}

// 0 the palm and the rest, 1 thumb, 2 index, 3 middle, 4 ring, 5 little finger
static int FingerOf(const std::string& n) {
    auto has = [&](const char* s) { return n.find(s) != std::string::npos; };
    if (has("Thumb") || has("_thumb")) return 1;
    if (has("Index") || has("_ind_")) return 2;
    if (has("Middle") || has("_mid_")) return 3;
    if (has("Ring") || has("_ring_")) return 4;
    if (has("Pinky") || has("_pinky_")) return 5;
    return 0;
}

struct Mesh {
    bool ok = false;
    int side = 1, handJoint = -1;
    std::vector<std::string> names;
    std::vector<M4> bind;                 // each joint's bind in the hand's bind frame
    std::vector<int> a, b;                // the rig bones it follows (-1: the hand itself)
    std::vector<float> mix;               // ...this far toward b
    std::vector<uint8_t> onHand;          // a joint of the hand (its bones or its muscles)
    std::vector<float> v, n;              // vertices, normals in the hand's bind frame
    std::vector<uint16_t> j;
    std::vector<float> w;
    std::vector<uint32_t> tri;
    std::vector<uint8_t> draw;            // per triangle
    std::vector<int> group;               // per vertex: FingerOf its heaviest joint
    std::map<int, M4> rigBind;            // a rig bone's bind in the hand's bind frame (its joint's, or derived)
    // skinned
    std::vector<float> sv, sn;
};

static std::map<std::string, int> g_rigIndex;

static void LoadNames(const char* path) {
    FILE* f = std::fopen(path, "r");
    if (!f) return;
    char line[256];
    while (std::fgets(line, sizeof line, f)) {
        int i; char nm[200];
        if (std::sscanf(line, "%d %199s", &i, nm) == 2) g_rigIndex[nm] = i;
    }
    std::fclose(f);
}

static int Rig(const std::string& n) {
    auto it = g_rigIndex.find(n);
    return it == g_rigIndex.end() ? -1 : it->second;
}

// A rig bone's pose in the hand bone's frame, as the buffer holds it now.
static M4 PoseOf(int side, int bone, int hand) {
    if (bone < 0 || bone == hand) return FromPose(Pose{});
    Pose p;
    InFrame(g_buf.data(), side, bone, hand, nullptr, p);
    return FromPose(p);
}

// Loads one side's mesh; the buffer must hold the rig's reference pose (for bones the mesh has no joint of).
static bool Load(Mesh& M, const char* path, int side, int hand) {
    FILE* f = std::fopen(path, "rb");
    if (!f) { std::printf("handmesh: no %s\n", path); return false; }
    char magic[4];
    uint32_t nj, nv, nt;
    if (std::fread(magic, 1, 4, f) != 4 || std::memcmp(magic, "HMS1", 4) != 0 || std::fread(&nj, 4, 1, f) != 1 ||
        std::fread(&nv, 4, 1, f) != 1 || std::fread(&nt, 4, 1, f) != 1) { std::fclose(f); return false; }
    M.side = side;
    std::vector<M4> ibm(nj);
    M.names.resize(nj);
    for (uint32_t i = 0; i < nj; ++i) {
        char nm[64];
        std::fread(nm, 1, 64, f);
        nm[63] = 0;
        M.names[i] = nm;
        std::fread(ibm[i].data(), 4, 16, f);
    }
    struct V { float p[3], n[3]; uint16_t j[8]; float w[8]; };
    std::vector<V> vs(nv);
    std::fread(vs.data(), sizeof(V), nv, f);
    M.tri.resize((size_t)nt * 3);
    std::fread(M.tri.data(), 4, M.tri.size(), f);
    std::fclose(f);
    const std::string handName = side ? "RightHand" : "LeftHand", pre = side ? "Right" : "Left", mp = side ? "r_" : "l_";
    for (uint32_t i = 0; i < nj; ++i) if (M.names[i] == handName) M.handJoint = (int)i;
    if (M.handJoint < 0) { std::printf("handmesh: no %s joint\n", handName.c_str()); return false; }
    const M4& bhInv = ibm[M.handJoint];
    // the exporter's Y-up glTF frame back to the game's Z-up one in the hand's frame too (it conjugated the matrices):
    // game (x, y, z) = glTF (x, -z, y)
    const M4 K = {1, 0, 0, 0, 0, 0, 1, 0, 0, -1, 0, 0, 0, 0, 0, 1}, Kt = {1, 0, 0, 0, 0, 0, -1, 0, 0, 1, 0, 0, 0, 0, 0, 1};
    const M4 toGame = Mul(K, bhInv);
    M.bind.resize(nj);
    for (uint32_t i = 0; i < nj; ++i) {
        M4 bi;
        Inverse(ibm[i], bi);
        M.bind[i] = Mul(Mul(K, Mul(bhInv, bi)), Kt);
    }
    // the rig bones each joint follows
    auto inHand = [&](int bone) {
        for (int a = bone, k = 0; a >= 0 && k < 64; a = g_VRBoneParent[a], ++k)
            if (a == hand) return true;
        return false;
    };
    M.a.assign(nj, -1); M.b.assign(nj, -1); M.mix.assign(nj, 0.0f); M.onHand.assign(nj, 0);
    for (uint32_t i = 0; i < nj; ++i) {
        const std::string& n = M.names[i];
        const int r = Rig(n);
        if (r >= 0 && inHand(r)) { M.a[i] = M.b[i] = r == hand ? -1 : r; M.onHand[i] = 1; continue; }
        auto rule = [&](const char* part, const char* from, const char* to, float k) {
            if (n.rfind(mp, 0) != 0 || n.find(part) == std::string::npos) return false;
            M.a[i] = from[0] ? Rig(pre + from) : -1;
            M.b[i] = to[0] ? Rig(pre + to) : -1;
            M.mix[i] = k;
            M.onHand[i] = 1;
            return true;
        };
        if (rule("_ind_knuckle", "InHandIndex", "HandIndex1", 0.5f) || rule("_mid_knuckle", "InHandMiddle", "HandMiddle1", 0.5f) ||
            rule("_ring_knuckle", "InHandRing", "HandRing1", 0.5f) || rule("_pinky_knuckle", "InHandPinky", "HandPinky1", 0.5f) ||
            rule("_pinkyBase", "InHandPinky", "HandPinky1", 0.25f) || rule("_thumb_side", "", "InHandThumb", 0.5f) ||
            rule("_thumb_thumb_A", "", "InHandThumb", 0.5f))
            continue;
    }
    // each rig bone used: its bind in the hand's bind frame -- its joint's, or (a bone the mesh lacks: the thumb's
    // metacarpal) a child joint's with the rig's reference between them
    auto bindOf = [&](int bone) {
        if (bone < 0 || M.rigBind.count(bone)) return;
        for (uint32_t i = 0; i < nj; ++i)
            if (Rig(M.names[i]) == bone) { M.rigBind[bone] = M.bind[i]; return; }
        for (uint32_t i = 0; i < nj; ++i) {
            const int r = Rig(M.names[i]);
            if (r >= 0 && g_VRBoneParent[r] == bone) {
                M4 refC = PoseOf(side, r, hand), refB = PoseOf(side, bone, hand), refCInv;
                Inverse(refC, refCInv);
                M.rigBind[bone] = Mul(M.bind[i], Mul(refCInv, refB));
                return;
            }
        }
        M.rigBind[bone] = PoseOf(side, bone, hand);
    };
    for (uint32_t i = 0; i < nj; ++i) { bindOf(M.a[i]); bindOf(M.b[i]); }
    // vertices into the hand's bind frame
    M.v.resize((size_t)nv * 3); M.n.resize((size_t)nv * 3); M.j.resize((size_t)nv * 8); M.w.resize((size_t)nv * 8);
    M.group.resize(nv);
    std::vector<float> handW(nv);
    for (uint32_t k = 0; k < nv; ++k) {
        Point(toGame, vs[k].p, &M.v[k * 3]);
        Dir(toGame, vs[k].n, &M.n[k * 3]);
        Unit(&M.n[k * 3]);
        float hw = 0.0f, best = -1.0f;
        int heavy = 0;
        for (int s = 0; s < 8; ++s) {
            M.j[k * 8 + s] = vs[k].j[s] < nj ? vs[k].j[s] : 0;
            M.w[k * 8 + s] = vs[k].j[s] < nj ? vs[k].w[s] : 0.0f;
            if (M.onHand[M.j[k * 8 + s]]) hw += M.w[k * 8 + s];
            if (M.w[k * 8 + s] > best) { best = M.w[k * 8 + s]; heavy = M.j[k * 8 + s]; }
        }
        handW[k] = hw;
        M.group[k] = FingerOf(M.names[heavy]);
    }
    M.draw.resize(nt);
    for (uint32_t t = 0; t < nt; ++t) {
        const float most = std::max({handW[M.tri[t * 3]], handW[M.tri[t * 3 + 1]], handW[M.tri[t * 3 + 2]]});
        M.draw[t] = most >= 0.5f;
    }
    // the frames agree: a metacarpal's place off the hand, in the mesh's bind and in the rig's reference
    {
        const int ix = Rig(pre + "InHandIndex"), px = Rig(pre + "HandPinky1");
        for (int bone : {ix, px}) {
            if (!M.rigBind.count(bone)) bindOf(bone);
            const M4& q = M.rigBind[bone];
            const M4 r = PoseOf(side, bone, hand);
            std::printf("handmesh %s: bone %d bind (%.4f %.4f %.4f) rig (%.4f %.4f %.4f)\n", path, bone, q[12], q[13], q[14],
                        r[12], r[13], r[14]);
        }
    }
    M.ok = true;
    std::printf("handmesh %s: %u joints, %u verts, %u tris\n", path, nj, nv, nt);
    return true;
}

// Skinned to the buffer's pose now.
static void Skin(Mesh& M, int hand) {
    const size_t nj = M.names.size();
    std::vector<M4> S(nj);
    std::map<int, M4> boneSkin;
    auto skinOf = [&](int bone) -> M4 {
        if (bone < 0) return FromPose(Pose{});
        auto it = boneSkin.find(bone);
        if (it != boneSkin.end()) return it->second;
        M4 inv;
        Inverse(M.rigBind[bone], inv);
        const M4 s = Mul(PoseOf(M.side, bone, hand), inv);
        boneSkin[bone] = s;
        return s;
    };
    for (size_t i = 0; i < nj; ++i) {
        if (!M.onHand[i]) { S[i] = FromPose(Pose{}); continue; }
        const M4 A = skinOf(M.a[i]), B = skinOf(M.b[i]);
        for (int k = 0; k < 16; ++k) S[i][k] = A[k] + (B[k] - A[k]) * M.mix[i];
    }
    const size_t nv = M.v.size() / 3;
    M.sv.assign(nv * 3, 0.0f);
    M.sn.assign(nv * 3, 0.0f);
    for (size_t k = 0; k < nv; ++k) {
        float* o = &M.sv[k * 3];
        float* on = &M.sn[k * 3];
        for (int s = 0; s < 8; ++s) {
            const float wt = M.w[k * 8 + s];
            if (wt <= 0.0f) continue;
            float p[3], d[3];
            Point(S[M.j[k * 8 + s]], &M.v[k * 3], p);
            Dir(S[M.j[k * 8 + s]], &M.n[k * 3], d);
            for (int c = 0; c < 3; ++c) { o[c] += p[c] * wt; on[c] += d[c] * wt; }
        }
        Unit(on);
    }
}

// The real skin's least clearance to the thing per finger (m; under 0 into it), over the drawn vertices.
static int g_deepJoint[6];
static void Clearance(const Mesh& M, const Scene& sc, float* out6) {
    for (int g = 0; g < 6; ++g) { out6[g] = 1e9f; g_deepJoint[g] = -1; }
    const size_t nv = M.sv.size() / 3;
    std::vector<uint8_t> used(nv, 0);
    for (size_t t = 0; t < M.draw.size(); ++t)
        if (M.draw[t]) used[M.tri[t * 3]] = used[M.tri[t * 3 + 1]] = used[M.tri[t * 3 + 2]] = 1;
    for (size_t k = 0; k < nv; ++k) {
        if (!used[k]) continue;
        float l[3];
        ToFrame(sc.obj, &M.sv[k * 3], l);
        const float d = sc.rq.lathe.knots >= 2 ? LatheSdf(sc.rq.lathe, l) : Sdf(sc.rq.shape, sc.rq.half, l);
        if (d < out6[M.group[k]]) {
            out6[M.group[k]] = d;
            int heavy = 0;
            float best = -1.0f;
            for (int s = 0; s < 8; ++s)
                if (M.w[k * 8 + s] > best) { best = M.w[k * 8 + s]; heavy = M.j[k * 8 + s]; }
            g_deepJoint[M.group[k]] = heavy;
        }
    }
}

// Drawn over a view the raymarch left (its depth per pixel along fwd from the plane 0.3 m behind the centre).
static void Raster(const Mesh& M, const float* centre, float extent, int W, std::vector<uint8_t>& img, int x0, int imgW,
                   const float* fwd, const float* right, const float* u, const float* Lw, std::vector<float>& depth, bool tint) {
    const size_t nv = M.sv.size() / 3;
    std::vector<float> X(nv), Y(nv), Z(nv);
    for (size_t k = 0; k < nv; ++k) {
        float r[3];
        Sub(&M.sv[k * 3], centre, r);
        X[k] = (Dot(r, right) / extent + 0.5f) * W - 0.5f;
        Y[k] = (0.5f - Dot(r, u) / extent) * W - 0.5f;
        Z[k] = Dot(r, fwd) + 0.3f;
    }
    for (size_t t = 0; t < M.draw.size(); ++t) {
        if (!M.draw[t]) continue;
        const uint32_t i0 = M.tri[t * 3], i1 = M.tri[t * 3 + 1], i2 = M.tri[t * 3 + 2];
        const float ax = X[i0], ay = Y[i0], bx = X[i1], by = Y[i1], cx = X[i2], cy = Y[i2];
        const float area = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
        if (std::fabs(area) < 1e-9f) continue;
        const int xmin = std::max(0, (int)std::floor(std::min({ax, bx, cx}))), xmax = std::min(W - 1, (int)std::ceil(std::max({ax, bx, cx})));
        const int ymin = std::max(0, (int)std::floor(std::min({ay, by, cy}))), ymax = std::min(W - 1, (int)std::ceil(std::max({ay, by, cy})));
        for (int y = ymin; y <= ymax; ++y)
            for (int x = xmin; x <= xmax; ++x) {
                const float px = (float)x, py = (float)y;
                float w0 = ((bx - px) * (cy - py) - (by - py) * (cx - px)) / area;
                float w1 = ((cx - px) * (ay - py) - (cy - py) * (ax - px)) / area;
                float w2 = 1.0f - w0 - w1;
                if (w0 < 0.0f || w1 < 0.0f || w2 < 0.0f) continue;
                const float z = w0 * Z[i0] + w1 * Z[i1] + w2 * Z[i2];
                float& dz = depth[(size_t)y * W + x];
                if (z >= dz) continue;
                dz = z;
                float nr[3];
                for (int c = 0; c < 3; ++c) nr[c] = w0 * M.sn[i0 * 3 + c] + w1 * M.sn[i1 * 3 + c] + w2 * M.sn[i2 * 3 + c];
                Unit(nr);
                if (Dot(nr, fwd) > 0.0f) for (float& c : nr) c = -c;
                const float diff = std::max(0.0f, Dot(nr, Lw)) * 0.75f + 0.25f;
                const int g = M.group[w0 >= w1 && w0 >= w2 ? i0 : w1 >= w2 ? i1 : i2];
                float col[3] = {0.86f, 0.66f, 0.55f};
                if (tint && g == 1) { col[0] = 0.92f; col[1] = 0.38f; col[2] = 0.32f; }
                if (tint && g == 5) { col[0] = 0.42f; col[1] = 0.78f; col[2] = 0.42f; }
                uint8_t* p = &img[((size_t)y * imgW + x0 + x) * 3];
                for (int c = 0; c < 3; ++c) p[c] = (uint8_t)std::clamp(col[c] * diff * 255.0f, 0.0f, 255.0f);
            }
    }
}

// Each segment's skin round its bone at the pose the buffer holds (the reference): the vertices mostly on the segment's
// joint, measured off the segment's axis -- toward `palm` (the palm's normal, square to the axis), away from it, to the
// sides -- and their radius percentiles (mm).
static void FitSegment(const Mesh& M, const char* joint, const float* a, const float* b, const float* palm, const char* tag) {
    int ji = -1;
    for (size_t i = 0; i < M.names.size(); ++i) if (M.names[i] == joint) ji = (int)i;
    if (ji < 0) { std::printf("fit %-8s no joint %s\n", tag, joint); return; }
    float ax[3];
    Sub(b, a, ax);
    const float len = Len(ax);
    Unit(ax);
    float pn[3];
    Mad(palm, ax, -Dot(palm, ax), pn);
    Unit(pn);
    float sumP = 0.0f, sumQ = 0.0f;
    int cnt = 0;
    float side[3];
    Cross(ax, pn, side);
    std::vector<float> rad;
    float toPalm = 0, toBack = 0, sideA = 0, sideB = 0;
    const size_t nv = M.sv.size() / 3;
    for (size_t k = 0; k < nv; ++k) {
        float wj = 0.0f;
        for (int s = 0; s < 8; ++s) if (M.j[k * 8 + s] == ji) wj += M.w[k * 8 + s];
        if (wj < 0.6f) continue;
        float r[3];
        Sub(&M.sv[k * 3], a, r);
        const float u = Dot(r, ax);
        if (u < 0.15f * len || u > 0.85f * len) continue;   // the middle of the segment: no knuckle bulges, no tip
        float perp[3];
        Mad(r, ax, -u, perp);
        rad.push_back(Len(perp));
        const float p = Dot(perp, pn), q = Dot(perp, side);
        sumP += p; sumQ += q; ++cnt;
        toPalm = std::max(toPalm, p);
        toBack = std::max(toBack, -p);
        sideA = std::max(sideA, q);
        sideB = std::max(sideB, -q);
    }
    if (rad.empty() || cnt == 0) { std::printf("fit %-8s %s: no vertices\n", tag, joint); return; }
    std::sort(rad.begin(), rad.end());
    auto pct = [&](float f) { return rad[std::min(rad.size() - 1, (size_t)(f * rad.size()))] * 1e3f; };
    std::printf("fit %-8s %-22s len %5.1f  n %4zu  r50 %4.1f r90 %4.1f max %4.1f  palm %4.1f back %4.1f sides %4.1f %4.1f"
                "  centre palm %4.1f side %4.1f mm\n",
                tag, joint, len * 1e3f, rad.size(), pct(0.5f), pct(0.9f), rad.back() * 1e3f, toPalm * 1e3f, toBack * 1e3f,
                sideA * 1e3f, sideB * 1e3f, sumP / cnt * 1e3f, sumQ / cnt * 1e3f);
    // along it, a tenth of its length a bin: the most toward the palm and to either side (mm), to the tip and past it
    float binP[12] = {}, binS[12] = {}, binB[12] = {};
    for (size_t k = 0; k < nv; ++k) {
        float wj = 0.0f;
        for (int s = 0; s < 8; ++s) if (M.j[k * 8 + s] == ji) wj += M.w[k * 8 + s];
        if (wj < 0.6f) continue;
        float r[3];
        Sub(&M.sv[k * 3], a, r);
        const int bin = (int)std::floor(Dot(r, ax) / len * 10.0f);
        if (bin < 0 || bin > 11) continue;
        float perp[3];
        Mad(r, ax, -Dot(r, ax), perp);
        binP[bin] = std::max(binP[bin], Dot(perp, pn));
        binS[bin] = std::max(binS[bin], std::fabs(Dot(perp, side)));
        binB[bin] = std::max(binB[bin], -Dot(perp, pn));
    }
    std::printf("    profile %-8s palm/side/back by tenths:", tag);
    for (int bin = 0; bin < 12; ++bin) std::printf(" %.1f/%.1f/%.1f", binP[bin] * 1e3f, binS[bin] * 1e3f, binB[bin] * 1e3f);
    std::printf("\n");
}

// The real palm's skin over the solver's palm plane: on a grid across the palm (d along the fingers, t toward the thumb,
// from pm.skin), the highest drawn vertex of the palm's side (its normal along +n) in each cell, mm.
static void PalmRelief(const Mesh& M, const Palm& pm) {
    const int ND = 9, NT = 9;
    float hi[ND][NT];
    for (auto& r : hi) for (float& x : r) x = -99.0f;
    const size_t nv = M.sv.size() / 3;
    std::vector<uint8_t> used(nv, 0);
    for (size_t t = 0; t < M.draw.size(); ++t)
        if (M.draw[t]) used[M.tri[t * 3]] = used[M.tri[t * 3 + 1]] = used[M.tri[t * 3 + 2]] = 1;
    for (size_t k = 0; k < nv; ++k) {
        if (!used[k] || Dot(&M.sn[k * 3], pm.n) < 0.3f) continue;
        float r[3];
        Sub(&M.sv[k * 3], pm.skin, r);
        const float d = Dot(r, pm.d), t = Dot(r, pm.t), h = Dot(r, pm.n);
        const int i = (int)std::floor((d + 0.045f) / 0.01f), j = (int)std::floor((t + 0.045f) / 0.01f);
        if (i < 0 || i >= ND || j < 0 || j >= NT) continue;
        hi[i][j] = std::max(hi[i][j], h * 1e3f);
    }
    std::printf("palm relief (mm over pm.skin along n; rows d from +0.045 down, cols t from -0.045 to +0.045 toward the thumb)\n");
    for (int i = ND - 1; i >= 0; --i) {
        std::printf("  d %+5.3f:", -0.045f + 0.01f * i + 0.005f);
        for (int j = 0; j < NT; ++j)
            if (hi[i][j] < -98.0f) std::printf("     .");
            else std::printf(" %5.1f", hi[i][j]);
        std::printf("\n");
    }
}

}  // namespace hm
