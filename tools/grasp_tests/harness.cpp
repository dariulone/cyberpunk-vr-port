// Offline harness for src/Anim/GraspPose.cpp: the live rig's reference hands (rigdump.py), a thing seated and the
// fingers closed on it exactly as in the game, then the solver's own model (finger capsules, palm slab, the thing)
// raymarched from four sides into a PPM.
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cmath>
#include <cstring>
#include <string>
#include <vector>
#include <algorithm>
#include <chrono>

int16_t g_VRBoneParent[800];
volatile int g_VRBoneCount = 0, g_VRRightBoneIdx = -1, g_VRLeftBoneIdx = -1, g_VRSmokeCigIdx = -1, g_VRSmokeLighterIdx = -1;

void VRIK_QuatRotateVec(const float* q, const float* v, float* o) {
    const float tx = 2.0f * (q[1] * v[2] - q[2] * v[1]);
    const float ty = 2.0f * (q[2] * v[0] - q[0] * v[2]);
    const float tz = 2.0f * (q[0] * v[1] - q[1] * v[0]);
    o[0] = v[0] + q[3] * tx + (q[1] * tz - q[2] * ty);
    o[1] = v[1] + q[3] * ty + (q[2] * tx - q[0] * tz);
    o[2] = v[2] + q[3] * tz + (q[0] * ty - q[1] * tx);
}
void VRIK_QuatMul(const float* a, const float* b, float* o) {
    o[0] = a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1];
    o[1] = a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0];
    o[2] = a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3];
    o[3] = a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2];
}
void VRIK_QuatConj(const float* q, float* o) { o[0] = -q[0]; o[1] = -q[1]; o[2] = -q[2]; o[3] = q[3]; }
void VRIK_QuatNorm(float* q) {
    float n = std::sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
    if (n > 1e-8f) { float inv = 1.0f / n; q[0] *= inv; q[1] *= inv; q[2] *= inv; q[3] *= inv; }
    else { q[0] = 0.0f; q[1] = 0.0f; q[2] = 0.0f; q[3] = 1.0f; }
}
static void Cross3(const float* a, const float* b, float* o) {
    o[0] = a[1] * b[2] - a[2] * b[1]; o[1] = a[2] * b[0] - a[0] * b[2]; o[2] = a[0] * b[1] - a[1] * b[0];
}
void VRIK_QuatFromTo(const float* a, const float* b, float* o) {
    float d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    if (d >= 1.0f - 1e-6f) { o[0] = 0.0f; o[1] = 0.0f; o[2] = 0.0f; o[3] = 1.0f; return; }
    if (d <= -1.0f + 1e-6f) {
        float ax[3] = {1.0f, 0.0f, 0.0f};
        if (std::fabs(a[0]) > 0.9f) { ax[0] = 0.0f; ax[1] = 1.0f; ax[2] = 0.0f; }
        float axis[3]; Cross3(a, ax, axis);
        float l = std::sqrt(axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]);
        o[0] = axis[0] / l; o[1] = axis[1] / l; o[2] = axis[2] / l; o[3] = 0.0f;
        return;
    }
    float c[3]; Cross3(a, b, c);
    o[0] = c[0]; o[1] = c[1]; o[2] = c[2]; o[3] = 1.0f + d;
    VRIK_QuatNorm(o);
}

static float s_ref[800][4];
static bool s_refOk[800];
static int s_fingers[2][19];
bool VRIK_ReferenceFingerRotation(int b, float* r) {
    if (b < 0 || b >= 800 || !s_refOk[b]) return false;
    std::memcpy(r, s_ref[b], 16);
    return true;
}
bool VRIK_ReferenceLocal(int, float*, float*) { return false; }   // the harness buffer is the reference already
int VRIK_LadderFingerBone(int side, int slot) { return (side < 0 || side > 1 || slot < 0 || slot >= 19) ? -1 : s_fingers[side][slot]; }

#include "C:/Users/dariulone/Desktop/CyberpunkVRPort/src/Anim/GraspPose.cpp"

// ---------------------------------------------------------------- the scene

static std::vector<uint8_t> g_buf;
static bool g_inTree[800];

struct Capsule { float a[3], b[3], r; };
struct Scene {
    std::vector<Capsule> caps;
    Palm pm;
    Request rq;
    Pose obj;   // hand frame
};

static float SegDist(const float* p, const Capsule& c) {
    float pa[3], ba[3];
    Sub(p, c.a, pa); Sub(c.b, c.a, ba);
    const float h = std::clamp(Dot(pa, ba) / std::max(Dot(ba, ba), 1e-12f), 0.0f, 1.0f);
    float d[3]; Mad(pa, ba, -h, d);
    return Len(d) - c.r;
}
// 0 nothing, 1 hand, 2 palm, 3 thing
static float Map(const Scene& s, const float* p, int& what) {
    float best = 1e9f; what = 0;
    for (const Capsule& c : s.caps) { float d = SegDist(p, c); if (d < best) { best = d; what = 1; } }
    { float d = PalmDist(s.pm, p) - 0.002f; if (d < best) { best = d; what = 2; } }
    float l[3]; ToFrame(s.obj, p, l);
    { float d = Sdf(s.rq.shape, s.rq.half, l); if (d < best) { best = d; what = 3; } }
    return best;
}

static void Render(const Scene& s, const float* centre, float extent, int W, std::vector<uint8_t>& img, int x0, int imgW,
                   const float* fwd, const float* up) {
    float right[3]; Cross(fwd, up, right); Unit(right);
    float u[3]; Cross(right, fwd, u); Unit(u);
    const float light[3] = {0.35f, 0.5f, 0.8f};
    float L[3] = {light[0], light[1], light[2]};
    // light from the camera's upper left
    float Lw[3];
    for (int k = 0; k < 3; ++k) Lw[k] = -fwd[k] * 0.7f + u[k] * 0.5f - right[k] * 0.4f;
    Unit(Lw);
    (void)L;
    for (int y = 0; y < W; ++y)
        for (int x = 0; x < W; ++x) {
            const float sx = ((x + 0.5f) / W - 0.5f) * extent, sy = (0.5f - (y + 0.5f) / W) * extent;
            float o[3];
            for (int k = 0; k < 3; ++k) o[k] = centre[k] + right[k] * sx + u[k] * sy - fwd[k] * 0.3f;
            float t = 0.0f; int what = 0; bool hit = false;
            for (int i = 0; i < 300 && t < 0.6f; ++i) {
                float p[3]; Mad(o, fwd, t, p);
                const float d = Map(s, p, what);
                if (d < 0.0002f) { hit = true; break; }
                t += std::max(d, 0.0002f);
            }
            uint8_t* px = &img[((size_t)y * imgW + x0 + x) * 3];
            if (!hit) { px[0] = 26; px[1] = 28; px[2] = 34; continue; }
            float p[3]; Mad(o, fwd, t, p);
            float nrm[3]; int w2;
            const float e = 0.0003f;
            for (int k = 0; k < 3; ++k) {
                float a[3] = {p[0], p[1], p[2]}, b[3] = {p[0], p[1], p[2]};
                a[k] += e; b[k] -= e;
                nrm[k] = Map(s, a, w2) - Map(s, b, w2);
            }
            Unit(nrm);
            const float diff = std::max(0.0f, Dot(nrm, Lw)) * 0.75f + 0.25f;
            float col[3] = {0.85f, 0.62f, 0.5f};
            if (what == 2) { col[0] = 0.75f; col[1] = 0.55f; col[2] = 0.45f; }
            if (what == 3) { col[0] = 0.35f; col[1] = 0.6f; col[2] = 0.95f; }
            for (int k = 0; k < 3; ++k) px[k] = (uint8_t)std::clamp(col[k] * diff * 255.0f, 0.0f, 255.0f);
        }
}

static void LoadRig(const char* path) {
    FILE* f = std::fopen(path, "r");
    if (!f) { std::printf("no rig %s\n", path); std::exit(1); }
    char line[512];
    for (auto& p : g_VRBoneParent) p = -1;
    while (std::fgets(line, sizeof line, f)) {
        char key[64];
        if (std::sscanf(line, "%63s", key) != 1) continue;
        std::string k = key;
        if (k == "count") { int c; std::sscanf(line, "%*s %d", &c); g_VRBoneCount = c; }
        else if (k == "g_VRRightBoneIdx") std::sscanf(line, "%*s %d", (int*)&g_VRRightBoneIdx);
        else if (k == "g_VRLeftBoneIdx") std::sscanf(line, "%*s %d", (int*)&g_VRLeftBoneIdx);
        else if (k == "g_VRSmokeCigIdx") std::sscanf(line, "%*s %d", (int*)&g_VRSmokeCigIdx);
        else if (k == "g_VRSmokeLighterIdx") std::sscanf(line, "%*s %d", (int*)&g_VRSmokeLighterIdx);
        else if (k == "fingers") {
            int side; char* p = line + 8;
            side = (int)std::strtol(p, &p, 10);
            for (int i = 0; i < 19; ++i) s_fingers[side][i] = (int)std::strtol(p, &p, 10);
        } else if (k == "bone") {
            int b, par, ok; float v[7];
            std::sscanf(line, "%*s %d %d %d %f %f %f %f %f %f %f", &b, &par, &ok, &v[0], &v[1], &v[2], &v[3], &v[4], &v[5], &v[6]);
            g_VRBoneParent[b] = (int16_t)par;
            s_refOk[b] = ok != 0;
            std::memcpy(s_ref[b], v + 3, 16);
            g_inTree[b] = true;
            float* t = reinterpret_cast<float*>(&g_buf[b * 48 + VRIK_TRANS_OFF]);
            float* q = reinterpret_cast<float*>(&g_buf[b * 48 + VRIK_ROT_OFF]);
            std::memcpy(t, v, 12); std::memcpy(q, v + 3, 16);
        }
    }
    std::fclose(f);
}

static void ResetBuffer() {
    for (int b = 0; b < 800; ++b) {
        if (!g_inTree[b]) continue;
        std::memcpy(&g_buf[b * 48 + VRIK_ROT_OFF], s_ref[b], 16);
    }
}

struct Case { std::string name; int side, shape; float half[3]; float q[4]; float off[2]; };

static void AddChain(Scene& sc, int side, int hand, const int* slots, const float* radius, bool thumb, int idx, const char* name) {
    int bones[3];
    for (int j = 0; j < 3; ++j) bones[j] = VRIK_LadderFingerBone(side, slots[j]);
    Pose P[3], base;
    InFrame(g_buf.data(), side, g_VRBoneParent[bones[0]], hand, nullptr, base);
    for (int j = 0; j < 3; ++j) InFrame(g_buf.data(), side, bones[j], hand, nullptr, P[j]);
    float tip[3], tr[3], T[3];
    TipOffset(g_buf.data(), bones[2], tip);
    Rot(P[2].q, tip, tr);
    for (int k = 0; k < 3; ++k) T[k] = P[2].p[k] + tr[k];
    const float* J[4] = {P[0].p, P[1].p, P[2].p, T};
    const float zero[3] = {0, 0, 0};
    if (!thumb) {
        Capsule m2{}; std::copy_n(zero, 3, m2.a); std::copy_n(base.p, 3, m2.b); m2.r = 0.011f;
        Capsule meta{}; std::copy_n(base.p, 3, meta.a); std::copy_n(P[0].p, 3, meta.b); meta.r = 0.010f;
        sc.caps.push_back(m2);
        sc.caps.push_back(meta);
    } else {
        Capsule m2{}; std::copy_n(zero, 3, m2.a); std::copy_n(P[0].p, 3, m2.b); m2.r = 0.013f;
        sc.caps.push_back(m2);
    }
    const volatile float* A = CyberpunkVR_DebugGraspAngles[side];
    if (thumb) std::printf("    %-6s lift %4.0f swing %4.0f mcp %4.0f ip %4.0f  gap", name, A[1], A[2], A[3], A[4]);
    else std::printf("    %-6s %5.0f %5.0f %5.0f                         gap", name, A[5 + idx * 3], A[6 + idx * 3], A[7 + idx * 3]);
    for (int s = 0; s < 3; ++s) {
        Capsule cp{}; std::copy_n(J[s], 3, cp.a); std::copy_n(J[s + 1], 3, cp.b); cp.r = radius[s];
        sc.caps.push_back(cp);
        float best = 1e9f;
        for (int u = 0; u <= 8; ++u) {
            float p[3];
            for (int k = 0; k < 3; ++k) p[k] = J[s][k] + (J[s + 1][k] - J[s][k]) * (u / 8.0f);
            float l[3]; ToFrame(sc.obj, p, l);
            best = std::min(best, Sdf(sc.rq.shape, sc.rq.half, l) - radius[s]);
        }
        std::printf(" %6.1f", best * 1000.0f);
    }
    std::printf(" mm\n");
}

int main(int argc, char** argv) {
    if (argc < 3) { std::printf("harness RIG OUTDIR [name side shape hx hy hz qx qy qz qw offD offT]...\n"); return 1; }
    g_buf.assign(800 * 48, 0);
    for (int b = 0; b < 800; ++b) reinterpret_cast<float*>(&g_buf[b * 48 + VRIK_ROT_OFF])[3] = 1.0f;
    LoadRig(argv[1]);
    std::vector<Case> cases;
    for (int i = 3; i + 11 < argc; i += 12) {
        Case c; c.name = argv[i]; c.side = std::atoi(argv[i + 1]); c.shape = std::atoi(argv[i + 2]);
        for (int k = 0; k < 3; ++k) c.half[k] = (float)std::atof(argv[i + 3 + k]);
        for (int k = 0; k < 4; ++k) c.q[k] = (float)std::atof(argv[i + 6 + k]);
        for (int k = 0; k < 2; ++k) c.off[k] = (float)std::atof(argv[i + 10 + k]);
        VRIK_QuatNorm(c.q);
        cases.push_back(c);
    }
    for (const Case& c : cases) {
        ResetBuffer();
        for (auto& s : g_side) { s.active = false; s.blend = 0.0f; s.palm = false; s.rev = 0; s.solved = ~0u; }
        cvr::anim::GraspFingers(g_buf.data());   // measures the palms
        Side& S = g_side[c.side];
        // the grab point: off the palm's centre along the fingers and toward the thumb, in the slot's frame
        float at[3], grab[3];
        Mad(S.pm.skin, S.pm.d, c.off[0], grab);
        Mad(grab, S.pm.t, c.off[1], grab);
        ToFrame(S.pm.slot, grab, at);
        Pose seat;
        if (!Seat(c.side, c.shape, c.half, at, c.q, seat)) { std::printf("%s: no seat\n", c.name.c_str()); continue; }
        S.req.shape = c.shape;
        std::copy_n(c.half, 3, S.req.half);
        S.req.obj = seat;
        S.active = true; ++S.rev; S.blend = 1.0f;
        const auto t0 = std::chrono::steady_clock::now();
        cvr::anim::GraspFingers(g_buf.data());
        const double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
        const int hand = c.side ? g_VRRightBoneIdx : g_VRLeftBoneIdx;
        Scene sc;
        sc.pm = S.pm; sc.rq = S.req; sc.obj = Compose(S.pm.slot, S.req.obj);
        int axis;
        const Kind kind = Classify(c.shape, c.half, axis);
        const volatile float* T = CyberpunkVR_DebugGraspThumb[c.side];
        std::printf("%-10s kind %d  %.2f ms  contacts %llu  thumb cost %.4f pen %.1f gap %.1f goal %.1f mm\n", c.name.c_str(),
                    (int)kind, ms, (unsigned long long)CyberpunkVR_DebugGrasp[3], T[0], T[1], T[2], T[3]);
        static const char* names[4] = {"index", "middle", "ring", "pinky"};
        // the palm's frame in the hand bone's: what the lobby's and the game's reach aim with (palm out along n)
        std::printf("    palm (hand frame): skin (%.4f %.4f %.4f) n (%.4f %.4f %.4f) d (%.4f %.4f %.4f) t (%.4f %.4f %.4f)\n",
                    S.pm.skin[0], S.pm.skin[1], S.pm.skin[2], S.pm.n[0], S.pm.n[1], S.pm.n[2],
                    S.pm.d[0], S.pm.d[1], S.pm.d[2], S.pm.t[0], S.pm.t[1], S.pm.t[2]);
        {
            float ref[kSlots][4];
            ReferenceFingers(g_buf.data(), c.side, ref, nullptr);
            ChainBones cb;
            if (ChainOf(g_buf.data(), c.side, hand, kThumbSlots, ref, cb)) {
                float J[4][3]; Pose F[3]; float rot[3][4];
                for (int j = 0; j < 3; ++j) std::copy_n(ref[kThumbSlots[j]], 4, rot[j]);
                Joints(cb, rot, J, F);
                std::printf("    thumb ref joints (n, d, t from the palm base):");
                for (int j = 0; j < 4; ++j) {
                    float r[3]; Sub(J[j], S.pm.base, r);
                    std::printf("  (%.3f %.3f %.3f)", Dot(r, S.pm.n), Dot(r, S.pm.d), Dot(r, S.pm.t));
                }
                std::printf("  halfD %.3f halfT %.3f\n", S.pm.halfD, S.pm.halfT);
            }
        }
        AddChain(sc, c.side, hand, kThumbSlots, kThumbRadius, true, 0, "thumb");
        for (int i = 0; i < 4; ++i) AddChain(sc, c.side, hand, kFingers[i].slot, kFingers[i].radius, false, i, names[i]);
        float centre[3];
        for (int k = 0; k < 3; ++k) centre[k] = 0.5f * (sc.pm.knuckle[k] + sc.obj.p[k]);
        const float extent = 0.26f;
        const int W = 320;
        std::vector<uint8_t> img((size_t)W * 4 * W * 3);
        float minusT[3], minusD[3], n[3], d[3], t[3];
        std::copy_n(sc.pm.n, 3, n); std::copy_n(sc.pm.d, 3, d); std::copy_n(sc.pm.t, 3, t);
        for (int k = 0; k < 3; ++k) { minusT[k] = -t[k]; minusD[k] = -d[k]; }
        Render(sc, centre, extent, W, img, 0, W * 4, minusT, n);       // from the thumb's side
        Render(sc, centre, extent, W, img, W, W * 4, t, n);            // from the little finger's side
        Render(sc, centre, extent, W, img, 2 * W, W * 4, minusD, n);   // from the fingertips
        float three[3];
        for (int k = 0; k < 3; ++k) three[k] = -n[k] * 0.6f - t[k] * 0.5f - d[k] * 0.4f;
        Unit(three);
        Render(sc, centre, extent, W, img, 3 * W, W * 4, three, d);    // into the palm, three quarters
        std::string path = std::string(argv[2]) + "/" + c.name + ".ppm";
        FILE* o = std::fopen(path.c_str(), "wb");
        std::fprintf(o, "P6\n%d %d\n255\n", W * 4, W);
        std::fwrite(img.data(), 1, img.size(), o);
        std::fclose(o);
    }
    return 0;
}
