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

int16_t g_VRBoneParent[2048];   // VRIK_MAX_BONES (include/Anim/VrikState.hpp)
volatile int g_VRBoneCount = 0, g_VRRightBoneIdx = -1, g_VRLeftBoneIdx = -1, g_VRSmokeCigIdx = -1, g_VRSmokeLighterIdx = -1;
void Log(const char*, ...) {}   // the plugin log: nothing offline
volatile int g_VRFKSnapCount = 0;       // no drawn hands offline: the world's up comes from GRASP_UP
volatile float g_VRFKSnapRot[2048][4] = {};

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

namespace hm { struct Mesh; }
struct Capsule { float a[3], b[3], r; int tag = 0; };   // tag 1: the thumb (drawn red), 2: the little finger (green)
struct Scene {
    std::vector<Capsule> caps;
    std::vector<Capsule> context;   // the whole thing round the part the hand closes on (a bottle), drawn grey
    Palm pm;
    Request rq;
    Pose obj;   // hand frame
    hm::Mesh* mesh = nullptr;   // the game's hand drawn instead of the capsules and the palm slab
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
    if (!s.mesh) {
        for (const Capsule& c : s.caps) { float d = SegDist(p, c); if (d < best) { best = d; what = c.tag == 1 ? 5 : c.tag == 2 ? 6 : 1; } }
        { float d = PalmDist(s.pm, p) - 0.002f; if (d < best) { best = d; what = 2; } }
    }
    float l[3]; ToFrame(s.obj, p, l);
    { float d = s.rq.lathe.knots >= 2 ? LatheSdf(s.rq.lathe, l) : Sdf(s.rq.shape, s.rq.half, l); if (d < best) { best = d; what = 3; } }
    for (const Capsule& c : s.context) { float d = SegDist(p, c); if (d < best) { best = d; what = 4; } }
    return best;
}

#include "handmesh.hpp"

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
    std::vector<float> depth((size_t)W * W, 1e9f);
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
            depth[(size_t)y * W + x] = t;
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
            if (what == 4) { col[0] = 0.55f; col[1] = 0.58f; col[2] = 0.6f; }
            if (what == 5) { col[0] = 0.9f; col[1] = 0.25f; col[2] = 0.2f; }
            if (what == 6) { col[0] = 0.3f; col[1] = 0.8f; col[2] = 0.35f; }
            for (int k = 0; k < 3; ++k) px[k] = (uint8_t)std::clamp(col[k] * diff * 255.0f, 0.0f, 255.0f);
        }
    if (s.mesh) {
        const char* tn = std::getenv("GRASP_TINT");
        hm::Raster(*s.mesh, centre, extent, W, img, x0, imgW, fwd, right, u, Lw, depth, !(tn && tn[0] == '0'));
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

struct Case { std::string name; int side, shape; float half[3]; float q[4]; float off[3]; };

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
    if (thumb) std::printf("    %-6s elev %4.0f out %4.0f mcp %4.0f ip %4.0f  gap", name, A[1], A[2], A[3], A[4]);
    else std::printf("    %-6s %5.0f %5.0f %5.0f                         gap", name, A[5 + idx * 3], A[6 + idx * 3], A[7 + idx * 3]);
    for (int s = 0; s < 3; ++s) {
        Capsule cp{}; std::copy_n(J[s], 3, cp.a); std::copy_n(J[s + 1], 3, cp.b); cp.r = radius[s];
        cp.tag = thumb ? 1 : idx == 3 ? 2 : 0;
        sc.caps.push_back(cp);
        float best = 1e9f;
        for (int u = 0; u <= 8; ++u) {
            float p[3];
            for (int k = 0; k < 3; ++k) p[k] = J[s][k] + (J[s + 1][k] - J[s][k]) * (u / 8.0f);
            float l[3]; ToFrame(sc.obj, p, l);
            best = std::min(best, (sc.rq.lathe.knots >= 2 ? LatheSdf(sc.rq.lathe, l) : Sdf(sc.rq.shape, sc.rq.half, l)) - radius[s]);
        }
        std::printf(" %6.1f", best * 1000.0f);
    }
    std::printf(" mm   joints");
    // its joints in the palm's frame (n, d, t off the palm's skin centre), mm
    for (int j = 0; j < 4; ++j) {
        float r[3];
        Sub(J[j], sc.pm.skin, r);
        std::printf(" (%.0f %.0f %.0f)", Dot(r, sc.pm.n) * 1e3f, Dot(r, sc.pm.d) * 1e3f, Dot(r, sc.pm.t) * 1e3f);
    }
    std::printf("\n");
}

int main(int argc, char** argv) {
    if (argc < 3) { std::printf("harness RIG OUTDIR [name side shape hx hy hz qx qy qz qw offD offT]...\n"); return 1; }
    g_buf.assign(800 * 48, 0);
    for (int b = 0; b < 800; ++b) reinterpret_cast<float*>(&g_buf[b * 48 + VRIK_ROT_OFF])[3] = 1.0f;
    LoadRig(argv[1]);
    // the game's hands (handmesh.py), loaded at the rig's reference
    static hm::Mesh meshes[2];
    const char* gm = std::getenv("GRASP_MESH");
    if (!(gm && gm[0] == '0')) {
        std::string dir = argv[1];
        const size_t cut = dir.find_last_of("/\\");
        dir = cut == std::string::npos ? std::string(".") : dir.substr(0, cut);
        hm::LoadNames((dir + "/mesh/rig_names.txt").c_str());
        ResetBuffer();
        hm::Load(meshes[1], (dir + "/mesh/hand_r.bin").c_str(), 1, g_VRRightBoneIdx);
        hm::Load(meshes[0], (dir + "/mesh/hand_l.bin").c_str(), 0, g_VRLeftBoneIdx);
    }
    std::vector<Case> cases;
    for (int i = 3; i + 12 < argc; i += 13) {
        Case c; c.name = argv[i]; c.side = std::atoi(argv[i + 1]); c.shape = std::atoi(argv[i + 2]);
        for (int k = 0; k < 3; ++k) c.half[k] = (float)std::atof(argv[i + 3 + k]);
        for (int k = 0; k < 4; ++k) c.q[k] = (float)std::atof(argv[i + 6 + k]);
        for (int k = 0; k < 3; ++k) c.off[k] = (float)std::atof(argv[i + 10 + k]);
        VRIK_QuatNorm(c.q);
        cases.push_back(c);
    }
    const bool regress = std::getenv("GRASP_REGRESS") != nullptr;
    int failures = 0;
    auto check = [&](bool ok, const Case& c, const char* text) {
        if (!ok) { std::printf("FAIL %s: %s\n", c.name.c_str(), text); ++failures; }
    };
    for (const Case& c : cases) {
        ResetBuffer();
        for (auto& s : g_side) {
            s.active = false; s.blend = 0.0f; s.palm = false; s.rev = 0; s.solved = ~0u;
            s.person = s.personOn = s.shownOn = false; s.style = 0;
            s.req = Request{};
        }
        cvr::anim::GraspFingers(g_buf.data());   // measures the palms
        Side& S = g_side[c.side];
        // the grab point: off the palm's centre along the fingers and toward the thumb, in the slot's frame
        float at[3], grab[3];
        Mad(S.pm.skin, S.pm.d, c.off[0], grab);
        Mad(grab, S.pm.t, c.off[1], grab);
        Mad(grab, S.pm.n, c.off[2], grab);
        // GRASP_AXIS=n: the thing turned so that its z axis lies along the palm's normal (a can taken by an end)
        float q[4] = {c.q[0], c.q[1], c.q[2], c.q[3]};
        if (const char* ax = std::getenv("GRASP_AXIS"); ax && (std::strcmp(ax, "n") == 0 || std::strcmp(ax, "-n") == 0)) {
            float nSlot[3];
            Unrot(S.pm.slot.q, S.pm.n, nSlot);
            if (ax[0] == '-') for (float& v : nSlot) v = -v;     // its +Z end toward the palm
            const float z[3] = {0.0f, 0.0f, 1.0f};
            VRIK_QuatFromTo(z, nSlot, q);
        }
        ToFrame(S.pm.slot, grab, at);
        int shape = c.shape;
        float half[3] = {c.half[0], c.half[1], c.half[2]};
        int ends = 15;
        bool part = false;
        cvr::anim::GraspLathe lathe;
        std::vector<Capsule> whole;     // in the thing's own frame
        float partZ = 0.0f;
        if (shape == 5) {
            // a bottle's hull: rings of 16 points -- the base, the body's top, the shoulder, the neck's foot, the mouth
            const float R = c.half[0], r = c.half[1], L = c.half[2];
            const float rings[6][2] = {{-L, R * 0.96f}, {-L + 0.01f, R}, {L * 0.15f, R}, {L * 0.42f, R * 0.72f}, {L * 0.55f, r * 1.05f}, {L, r * 1.12f}};
            std::vector<float> pts;
            for (const auto& g : rings)
                for (int k = 0; k < 16; ++k) {
                    const float a = 6.2831853f * k / 16.0f;
                    pts.push_back(g[1] * std::cos(a)); pts.push_back(g[1] * std::sin(a)); pts.push_back(g[0]);
                }
            const char* gz = std::getenv("GRASP_Z");
            const float zg = gz ? (float)std::atof(gz) : 0.0f;
            // the bottle's middle: its point zg under the grab point
            float zOff[3], zAx[3];
            const float ez[3] = {0.0f, 0.0f, 1.0f};
            Rot(q, ez, zAx);
            for (int k = 0; k < 3; ++k) zOff[k] = zAx[k] * zg;
            Sub(at, zOff, at);
            // the palm in the bottle's frame
            float skinSlot[3], rel[3], hand[3];
            ToFrame(S.pm.slot, S.pm.skin, skinSlot);
            Sub(skinSlot, at, rel);
            Unrot(q, rel, hand);
            cvr::anim::GraspPart gp;
            if (cvr::anim::GraspPartOf(pts.data(), (int)pts.size() / 3, L, hand, gp)) {
                shape = 2; half[0] = half[1] = gp.radius; half[2] = gp.half; ends = gp.ends; part = true; partZ = gp.z;
                if (!std::getenv("GRASP_NOLATHE")) lathe = gp.lathe;
                for (int k = 0; k < 3; ++k) at[k] += zAx[k] * gp.z;
                std::printf("%s: the %s, z %.3f half %.3f radius %.3f, ends %d (palm at z %.3f)\n", c.name.c_str(),
                            gp.neck ? "neck" : "body", gp.z, gp.half, gp.radius, gp.ends, hand[2]);
            } else {
                shape = 2; half[0] = half[1] = R; half[2] = L;
                std::printf("%s: no neck found, the whole\n", c.name.c_str());
            }
            whole.push_back({{0.0f, 0.0f, -L + R}, {0.0f, 0.0f, L * 0.15f}, R});
            whole.push_back({{0.0f, 0.0f, L * 0.15f}, {0.0f, 0.0f, L * 0.42f}, R * 0.86f});
            whole.push_back({{0.0f, 0.0f, L * 0.42f}, {0.0f, 0.0f, L - r}, r * 1.05f});
        }
        Pose seat;
        // GRASP_UP: the world's up as the palm faces -- 'down' the palm facing down (the hand above), 'up' facing up
        float up[3];
        const char* how = std::getenv("GRASP_UP");
        const bool upKnown = how && (std::strcmp(how, "down") == 0 || std::strcmp(how, "up") == 0);
        if (upKnown) for (int k = 0; k < 3; ++k) up[k] = std::strcmp(how, "down") == 0 ? -S.pm.n[k] : S.pm.n[k];
        int seatKind = -1;
        // A PERSON'S PART (shape 10 + the style, GraspPerson): a capsule (half: its radius, -, its half length) on the
        // palm's skin at the grab point, across the palm (t) or along the fingers (GRASP_PAXIS=d); no seat
        const bool person = c.shape >= 10;
        if (person) {
            const char* pa = std::getenv("GRASP_PAXIS");
            float axisHand[3], centre[3], qz[4];
            std::copy_n(pa && pa[0] == 'd' ? S.pm.d : S.pm.t, 3, axisHand);
            Mad(grab, S.pm.n, c.half[0], centre);
            const float z[3] = {0.0f, 0.0f, 1.0f};
            VRIK_QuatFromTo(z, axisHand, qz);
            shape = 4;
            half[0] = half[1] = c.half[0];
            half[2] = c.half[2];
            const char* captured = std::getenv("GRASP_CAPTURE");
            if (captured) {
                // Exact live hand-frame capsule: solve without another seat pass changing the evidence.
                float liveP[3], liveAxis[3];
                FILE* in = std::fopen(captured, "r");
                if (!in || std::fscanf(in, "%f %f %f %f %f %f %f %f", &liveP[0], &liveP[1], &liveP[2],
                    &liveAxis[0], &liveAxis[1], &liveAxis[2], &half[0], &half[2]) != 8) {
                    std::fprintf(stderr, "invalid GRASP_CAPTURE\n"); if (in) std::fclose(in); return 2;
                }
                std::fclose(in);half[1]=half[0];Unit(liveAxis);VRIK_QuatFromTo(z,liveAxis,qz);
                Pose live;std::copy_n(liveP,3,live.p);std::copy_n(qz,4,live.q);
                ToFrame(S.pm.slot,live.p,S.req.obj.p);float inv[4];VRIK_QuatConj(S.pm.slot.q,inv);
                VRIK_QuatMul(inv,live.q,S.req.obj.q);S.req.shape=kCapsule;std::copy_n(half,3,S.req.half);
                S.req.force=kWrap;S.req.part=true;S.person=S.personOn=S.active=true;++S.rev;
            } else if (c.name == "p_live_fixed") {
                // Captured right-hand corpse shin, already seated by the live caller. Do not reseat it.
                const float liveP[3] = {-0.1004694775f, -0.0032533500f, 0.0738972649f};
                float liveAxis[3] = {-0.4740791263f, 0.8793999242f, -0.0436628239f};
                Unit(liveAxis);
                VRIK_QuatFromTo(z, liveAxis, qz);
                Pose live;
                std::copy_n(liveP, 3, live.p);
                std::copy_n(qz, 4, live.q);
                ToFrame(S.pm.slot, live.p, S.req.obj.p);
                float inv[4];
                VRIK_QuatConj(S.pm.slot.q, inv);
                VRIK_QuatMul(inv, live.q, S.req.obj.q);
                S.req.shape = kCapsule;
                S.req.half[0] = S.req.half[1] = c.half[0];
                S.req.half[2] = c.half[2];
                S.req.force = kWrap;
                S.req.part = true;
                S.person = S.personOn = S.active = true;
                ++S.rev;
            } else if (!cvr::anim::GraspPerson(c.side, c.shape - 10, centre, qz, c.half[2], c.half[0])) { std::printf("%s: refused\n", c.name.c_str()); continue; }
        } else {
            if (!Seat(c.side, shape, half, at, q, upKnown ? up : nullptr, seat, &seatKind, ends, part, &lathe)) { std::printf("%s: no seat\n", c.name.c_str()); continue; }
            S.req.shape = shape;
            std::copy_n(half, 3, S.req.half);
            S.req.obj = seat;
            S.req.ends = ends;
            S.req.part = part;
            S.req.lathe = lathe;
            S.req.force = -1; S.req.limit = 1.0f; S.req.spread = -1.0f;
            S.active = true; ++S.rev;
        }
        // Optional seat perturbation for offline clearance diagnosis, in the palm frame (metres).
        if (person) {
            Pose pose = Compose(S.pm.slot, S.req.obj);
            const char* env[3] = {"GRASP_SEAT_D", "GRASP_SEAT_N", "GRASP_SEAT_T"};
            const float* axes[3] = {S.pm.d, S.pm.n, S.pm.t};
            for (int k = 0; k < 3; ++k)
                if (const char* value = std::getenv(env[k])) Mad(pose.p, axes[k], (float)std::atof(value), pose.p);
            ToFrame(S.pm.slot, pose.p, S.req.obj.p);
        }
        S.blend = 1.0f;
        const auto t0 = std::chrono::steady_clock::now();
        cvr::anim::GraspFingers(g_buf.data());
        const double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
        const int hand = c.side ? g_VRRightBoneIdx : g_VRLeftBoneIdx;
        Scene sc;
        sc.pm = S.pm; sc.rq = S.req; sc.obj = Compose(S.pm.slot, S.req.obj);
        int axis;
        const Kind kind = person ? static_cast<Kind>(S.req.force) : Seated(shape, half, Compose(S.pm.slot, S.req.obj), S.pm, axis, part);
        const bool fixed = c.name == "p_live_fixed";
        if (regress) {
            for (int s = 0; s < kSlots; ++s)
                check(std::isfinite(cvr::anim::Dot4(S.rot[s], S.rot[s])) && std::fabs(cvr::anim::Dot4(S.rot[s], S.rot[s]) - 1.0f) < 0.001f,
                      c, "non-unit or non-finite solved rotation");
            if (person) check(std::fabs(S.req.half[0] - c.half[0]) < 1e-6f && std::fabs(S.req.half[2] - c.half[2]) < 1e-6f,
                              c, "limb dimensions changed");
            if (person && kind == kWrap && !fixed) {
                check(CyberpunkVR_DebugGraspAngles[c.side][2] <= 66.0f, c, "thumb abducted too widely");
                for (int f = 0; f < 4; ++f)
                    check(CyberpunkVR_DebugGraspAngles[c.side][6 + f * 3] >= 20.0f, c, "flat PIP in a seated limb wrap");
            }
            if (fixed) {
                check(CyberpunkVR_DebugGraspAngles[c.side][6] < 1.0f, c, "unsafe pre-curl accepted through fixed limb");
                std::printf("    LIMIT fixed old seat: no reseating; index MCP/PIP remain blocked by the capsule\n");
            }
            check(meshes[c.side].ok, c, "real FPP mesh required for regressions");
        }
        // the whole bottle round the part, in the hand's frame
        for (const Capsule& w : whole) {
            if (lathe.knots >= 2) break;
            Capsule hc{};
            float a[3] = {w.a[0], w.a[1], w.a[2] - partZ}, b[3] = {w.b[0], w.b[1], w.b[2] - partZ};
            Rot(sc.obj.q, a, hc.a); Rot(sc.obj.q, b, hc.b); for (int k = 0; k < 3; ++k) { hc.a[k] += sc.obj.p[k]; hc.b[k] += sc.obj.p[k]; } hc.r = w.r;
            sc.context.push_back(hc);
        }
        if (!person && seatKind != (int)kind) std::printf("%s: seated as %d, solved as %d\n", c.name.c_str(), seatKind, (int)kind);
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
        // Evaluate finger/palm contact against the visible surface independently of the solver's clearance envelope.
        if (person) if (const char* visible = std::getenv("GRASP_VISIBLE_RADIUS")) sc.rq.half[0]=sc.rq.half[1]=float(std::atof(visible));
        if (meshes[c.side].ok) {
            hm::Skin(meshes[c.side], hand);
            sc.mesh = &meshes[c.side];
            float cl[6];
            hm::Clearance(meshes[c.side], sc, cl);
            if (regress && person && kind == kWrap) {
                for (int g = 0; g < 6; ++g) {
                    // The captured old seat has immutable knuckle overlap. Keep this explicit, not a false success.
                    const float minimum = fixed ? -0.0025f : -0.00025f;
                    check(std::isfinite(cl[g]) && cl[g] >= minimum, c, "skin penetration exceeds regression limit");
                    if (!fixed && g > 0) check(cl[g] < 0.006f, c, "finger no longer contacts the limb");
                }
            }
            std::printf("    skin   palm %5.1f thumb %5.1f index %5.1f middle %5.1f ring %5.1f pinky %5.1f mm (under 0: into it)\n",
                        cl[0] * 1e3f, cl[1] * 1e3f, cl[2] * 1e3f, cl[3] * 1e3f, cl[4] * 1e3f, cl[5] * 1e3f);
            std::printf("    deepest:");
            for (int g = 0; g < 6; ++g)
                std::printf(" %s", hm::g_deepJoint[g] >= 0 ? meshes[c.side].names[hm::g_deepJoint[g]].c_str() : "-");
            std::printf("\n");
            if (std::getenv("GRASP_DEEP")) {
                for (int g = 0; g < 6; ++g) {
                    float best = 1e9f; size_t at = 0;
                    for (size_t v = 0; v < meshes[c.side].group.size(); ++v) {
                        if (meshes[c.side].group[v] != g) continue;
                        float p[3]; ToFrame(sc.obj, &meshes[c.side].sv[v * 3], p);
                        const float gap = Sdf(sc.rq.shape, sc.rq.half, p);
                        if (gap < best) { best = gap; at = v; }
                    }
                    float rel[3]; Sub(&meshes[c.side].sv[at * 3], sc.pm.skin, rel);
                    std::printf("    vertex %d id %zu ndt %.1f %.1f %.1f weights", g, at,
                                Dot(rel, sc.pm.n)*1e3f, Dot(rel, sc.pm.d)*1e3f, Dot(rel, sc.pm.t)*1e3f);
                    for (int w = 0; w < 8; ++w)
                        if (meshes[c.side].w[at * 8 + w] > 0.01f)
                            std::printf(" %s=%.2f", meshes[c.side].names[meshes[c.side].j[at * 8 + w]].c_str(), meshes[c.side].w[at * 8 + w]);
                    std::printf("\n");
                }
            }
            if (std::getenv("GRASP_FIT")) {
                // at the rig's reference: the buffer back to it, skinned, each chain's segments measured
                const std::vector<float> keepV = meshes[c.side].sv, keepN = meshes[c.side].sn;
                ResetBuffer();
                hm::Skin(meshes[c.side], hand);
                const std::string pre = c.side ? "Right" : "Left";
                const char* fn[5] = {"Thumb", "Index", "Middle", "Ring", "Pinky"};
                hm::PalmRelief(meshes[c.side], sc.pm);
                if (std::getenv("GRASP_WEB_FIT")) {
                    hm::Mesh& M = meshes[c.side];
                    std::vector<std::array<float, 3>> picked;
                    for (size_t v = 0; v < M.group.size(); ++v) {
                        float rel[3]; Sub(&M.sv[v * 3], sc.pm.skin, rel);
                        const float n = Dot(rel, sc.pm.n), d = Dot(rel, sc.pm.d), t = Dot(rel, sc.pm.t);
                        if (n < 0.006f || d < -0.025f || d > 0.045f || t < 0.010f || t > 0.055f) continue;
                        float weights[3] = {};
                        for (int w = 0; w < 8; ++w) {
                            const int ji = M.j[v * 8 + w];
                            const int bones[3] = {s_fingers[c.side][0], s_fingers[c.side][5], s_fingers[c.side][6]};
                            for (int b = 0; b < 3; ++b) {
                                if (M.a[ji] == bones[b]) weights[b] += M.w[v * 8 + w] * (1.0f - M.mix[ji]);
                                if (M.b[ji] == bones[b]) weights[b] += M.w[v * 8 + w] * M.mix[ji];
                            }
                        }
                        if (weights[0] + weights[1] < 0.05f) continue;
                        bool near = false;
                        for (const auto& p : picked) {
                            float dist[3]; Sub(&M.sv[v * 3], p.data(), dist);
                            if (Len(dist) < 0.007f) near = true;
                        }
                        if (near) continue;
                        picked.push_back({M.sv[v * 3], M.sv[v * 3 + 1], M.sv[v * 3 + 2]});
                        std::printf("WEB {%.5ff, %.5ff, %.5ff, %.5ff, %.5ff, %.5ff},\n", n, d, t, weights[0], weights[1], weights[2]);
                    }
                }
                float ref[kSlots][4];
                ReferenceFingers(g_buf.data(), c.side, ref, nullptr);
                for (int f = 0; f < 5; ++f) {
                    const int* slots = f == 0 ? kThumbSlots : kFingers[f - 1].slot;
                    // the solver's own chain and hinges at the reference
                    ChainBones cb;
                    if (!ChainOf(g_buf.data(), c.side, hand, slots, ref, cb)) continue;
                    float rot0[3][4], J[4][3], hinge[3][3];
                    Pose F[3];
                    for (int j = 0; j < 3; ++j) std::copy_n(cb.local[j].q, 4, rot0[j]);
                    Joints(cb, rot0, J, F);
                    float toward[3];
                    if (f == 0) Sub(sc.pm.skin, J[1], toward);
                    else std::copy_n(sc.pm.n, 3, toward);
                    if (!Hinges(J, F, toward, hinge)) continue;
                    for (int s = 0; s < 3; ++s) {
                        float ax[3], hw[3], palmDir[3];
                        Sub(J[s + 1], J[s], ax);
                        Unit(ax);
                        Rot(F[s].q, hinge[s], hw);
                        Cross(hw, ax, palmDir);
                        Unit(palmDir);
                        std::string jn = f == 0 ? (s == 0 ? pre + "InHandThumb" : pre + "HandThumb" + std::to_string(s))
                                                : pre + "Hand" + fn[f] + std::to_string(s + 1);
                        const std::string tag = std::string(fn[f]) + std::to_string(s);
                        hm::FitSegment(meshes[c.side], jn.c_str(), J[s], J[s + 1], palmDir, tag.c_str());
                    }
                }
                meshes[c.side].sv = keepV;
                meshes[c.side].sn = keepN;
            }
        }
        if (std::getenv("GRASP_NO_RENDER")) continue;
        float centre[3];
        for (int k = 0; k < 3; ++k) centre[k] = 0.5f * (sc.pm.knuckle[k] + sc.obj.p[k]);
        const float extent = 0.26f;
        const int W = std::getenv("GRASP_W") ? std::atoi(std::getenv("GRASP_W")) : 320;
        std::vector<uint8_t> img((size_t)W * 5 * W * 3);
        float minusT[3], minusD[3], n[3], d[3], t[3];
        std::copy_n(sc.pm.n, 3, n); std::copy_n(sc.pm.d, 3, d); std::copy_n(sc.pm.t, 3, t);
        for (int k = 0; k < 3; ++k) { minusT[k] = -t[k]; minusD[k] = -d[k]; }
        Render(sc, centre, extent, W, img, 0, W * 5, minusT, n);       // from the thumb's side
        Render(sc, centre, extent, W, img, W, W * 5, t, n);            // from the little finger's side
        Render(sc, centre, extent, W, img, 2 * W, W * 5, minusD, n);   // from the fingertips
        float three[3];
        for (int k = 0; k < 3; ++k) three[k] = -n[k] * 0.6f - t[k] * 0.5f - d[k] * 0.4f;
        Unit(three);
        Render(sc, centre, extent, W, img, 3 * W, W * 5, three, d);    // into the palm, three quarters
        Render(sc, centre, extent, W, img, 4 * W, W * 5, n, d);        // from the back of the hand
        std::string path = std::string(argv[2]) + "/" + c.name + ".ppm";
        FILE* o = std::fopen(path.c_str(), "wb");
        std::fprintf(o, "P6\n%d %d\n255\n", W * 5, W);
        std::fwrite(img.data(), 1, img.size(), o);
        std::fclose(o);
    }
    if (regress) std::printf("REGRESSION %s: %zu cases, %d failures\n", failures ? "FAIL" : "PASS", cases.size(), failures);
    return failures ? 1 : 0;
}
