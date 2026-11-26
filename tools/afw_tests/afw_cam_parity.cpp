// AFW's plugin cameras, the C++ reference for the Rust port (rust/crates/cvr-render/src/afw_cameras.rs, test
// parity_with_the_cpp_cameras): Inverse, ToMat, WorldToView and ViewToClip copied verbatim from src/Render/Afw.cpp's
// reference branch, fed the same pseudo-random cases, every output float's bits hashed (FNV-1a) and printed.
// Build like the DLL: cl /nologo /O2 /EHsc /std:c++20 afw_cam_parity.cpp
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>

struct XrFovf { float angleLeft, angleRight, angleUp, angleDown; };
namespace cvr::framegen {
struct CameraData {
    float jitter[2]{},motionScale[2]{};
    float position[3]{},up[3]{},right[3]{},forward[3]{};
    float nearPlane{},farPlane{},verticalFov{},aspect{};
    float shift[2]{};
    bool inverted{},infinite{},jittered{},reset{},cameraMotion{},motion3d{};
};
}
namespace pd { struct Mat4 { float m[16]; }; }

struct M4 { double m[16]{}; double& at(int r, int c) { return m[c * 4 + r]; } double at(int r, int c) const { return m[c * 4 + r]; } };

M4 Inverse(const M4& a) {
    const double* m = a.m;
    double inv[16];
    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
    double det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    M4 o;
    if (!(std::fabs(det) > 1e-300)) return o;
    det = 1.0 / det;
    for (int i = 0; i < 16; ++i) o.m[i] = inv[i] * det;
    return o;
}

pd::Mat4 ToMat(const M4& a) {
    pd::Mat4 o{};
    for (int i = 0; i < 16; ++i) o.m[i] = float(a.m[i]);
    return o;
}

M4 WorldToView(const float position[3], const cvr::framegen::CameraData& c) {
    const double ax[3][3] = {{c.right[0], c.right[1], c.right[2]},
                             {c.up[0], c.up[1], c.up[2]},
                             {-c.forward[0], -c.forward[1], -c.forward[2]}};
    M4 v;
    for (int r = 0; r < 3; ++r) {
        double t = 0;
        for (int k = 0; k < 3; ++k) {
            v.at(r, k) = ax[r][k];
            t += ax[r][k] * position[k];
        }
        v.at(r, 3) = -t;
    }
    v.at(3, 3) = 1;
    return v;
}

M4 ViewToClip(const XrFovf& fov, const cvr::framegen::CameraData& c) {
    const double l = std::tan(fov.angleLeft), r = std::tan(fov.angleRight), u = std::tan(fov.angleUp), d = std::tan(fov.angleDown);
    const double n = c.nearPlane > 0 ? c.nearPlane : 0.02, f = c.farPlane > n ? c.farPlane : 16000.0;
    // depth = A + B / z for the forward distance z
    double A, B;
    if (c.infinite) { if (c.inverted) { A = 0; B = n; } else { A = 1; B = -n; } }
    else if (c.inverted) { B = n * f / (f - n); A = -n / (f - n); }
    else { B = -n * f / (f - n); A = f / (f - n); }
    M4 p;   // view z is backward: forward distance = -z
    p.at(0, 0) = 2.0 / (r - l);
    p.at(0, 2) = (r + l) / (r - l);
    p.at(1, 1) = 2.0 / (u - d);
    p.at(1, 2) = (u + d) / (u - d);
    p.at(2, 2) = -A;
    p.at(2, 3) = B;
    p.at(3, 2) = -1;
    return p;
}

static uint64_t s = 0x9E3779B97F4A7C15ull;
static uint64_t step() { s = s * 6364136223846793005ull + 1442695040888963407ull; return s; }
static float next(float lo, float hi) { const double u = double(step() >> 11) / 9007199254740992.0; return float(lo + (hi - lo) * u); }

int main() {
    uint64_t h = 1469598103934665603ull;
    auto mix = [&](const pd::Mat4& m) {
        for (float f : m.m) {
            uint32_t b;
            std::memcpy(&b, &f, 4);
            for (int i = 0; i < 4; ++i) { h ^= (b >> (8 * i)) & 0xFF; h *= 1099511628211ull; }
        }
    };
    for (int i = 0; i < 20000; ++i) {
        cvr::framegen::CameraData c{};
        for (int k = 0; k < 3; ++k) c.position[k] = next(-100.0f, 100.0f);
        for (int k = 0; k < 3; ++k) c.right[k] = next(-1.0f, 1.0f);
        for (int k = 0; k < 3; ++k) c.up[k] = next(-1.0f, 1.0f);
        for (int k = 0; k < 3; ++k) c.forward[k] = next(-1.0f, 1.0f);
        c.nearPlane = next(-0.1f, 0.5f);
        c.farPlane = next(-1.0f, 20000.0f);
        const uint64_t bits = step();
        c.inverted = (bits & 1) != 0;
        c.infinite = (bits & 6) == 6;
        const XrFovf fov{next(-1.2f, -0.3f), next(0.3f, 1.2f), next(0.3f, 1.2f), next(-1.2f, -0.3f)};
        float other[3];
        for (int k = 0; k < 3; ++k) other[k] = next(-100.0f, 100.0f);
        const M4 sv = WorldToView(c.position, c), dv = WorldToView(other, c), sp = ViewToClip(fov, c);
        mix(ToMat(sv));
        mix(ToMat(Inverse(sv)));
        mix(ToMat(dv));
        mix(ToMat(Inverse(dv)));
        mix(ToMat(sp));
        mix(ToMat(Inverse(sp)));
    }
    std::printf("%016llx\n", static_cast<unsigned long long>(h));
}
