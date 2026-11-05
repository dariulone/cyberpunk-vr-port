// shellfit: the rigid pose that lays one point cloud onto another (trimmed ICP from PCA starts).
//
//   shellfit.exe SOURCE.bin TARGET.bin [TRIM]
//
// SOURCE and TARGET are float32 xyz triples (little endian, no header). The answer is R, t with
// TARGET ~= R * SOURCE + t, printed as one JSON object: the rotation as a quaternion { i, j, k, r }, the translation,
// the trimmed RMS of the kept pairs and the share of source points that found a target point within 2 mm.
//
// Used by make_holster_shell.py: SOURCE is a weapon's own mesh (its frame: the frame the hand holds it in), TARGET the
// same weapon as a holster mod posed it in its holster (the body's model frame), so the answer is the weapon's pose
// in the holster -- what carries the holster's shell onto the port's own place for that weapon.
//
// The meshes are not vertex-for-vertex the same (a mod re-exports them, splits or merges parts), so the fit pairs
// every source point with its nearest target point and drops the worst TRIM share (default 0.15) of the pairs each
// round. It starts from each proper rotation that maps the source's principal axes onto the target's and keeps the
// start that ends lowest.
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <unordered_map>
#include <vector>

using V3 = std::array<double, 3>;
using M3 = std::array<std::array<double, 3>, 3>;

static std::vector<V3> Load(const char* path) {
    std::vector<V3> out;
    FILE* f = std::fopen(path, "rb");
    if (!f) { std::fprintf(stderr, "cannot open %s\n", path); std::exit(2); }
    float p[3];
    while (std::fread(p, sizeof(float), 3, f) == 3) out.push_back({p[0], p[1], p[2]});
    std::fclose(f);
    return out;
}

static V3 Mul(const M3& m, const V3& v) {
    return {m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2], m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
            m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2]};
}

static M3 MulM(const M3& a, const M3& b) {
    M3 r{};
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j)
            for (int k = 0; k < 3; ++k) r[i][j] += a[i][k] * b[k][j];
    return r;
}

static M3 T(const M3& a) {
    M3 r{};
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j) r[i][j] = a[j][i];
    return r;
}

static double Det(const M3& m) {
    return m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) +
           m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
}

// Eigen decomposition of a symmetric n x n matrix (cyclic Jacobi): values in `w`, vectors in the columns of `v`.
template <int N>
static void Jacobi(std::array<std::array<double, N>, N> a, std::array<double, N>& w,
                   std::array<std::array<double, N>, N>& v) {
    for (int i = 0; i < N; ++i)
        for (int j = 0; j < N; ++j) v[i][j] = i == j ? 1.0 : 0.0;
    for (int sweep = 0; sweep < 64; ++sweep) {
        double off = 0;
        for (int i = 0; i < N; ++i)
            for (int j = i + 1; j < N; ++j) off += a[i][j] * a[i][j];
        if (off < 1e-30) break;
        for (int p = 0; p < N; ++p)
            for (int q = p + 1; q < N; ++q) {
                if (std::fabs(a[p][q]) < 1e-300) continue;
                const double th = (a[q][q] - a[p][p]) / (2 * a[p][q]);
                const double t = (th >= 0 ? 1.0 : -1.0) / (std::fabs(th) + std::sqrt(th * th + 1));
                const double c = 1 / std::sqrt(t * t + 1), s = t * c;
                for (int k = 0; k < N; ++k) {
                    const double akp = a[k][p], akq = a[k][q];
                    a[k][p] = c * akp - s * akq;
                    a[k][q] = s * akp + c * akq;
                }
                for (int k = 0; k < N; ++k) {
                    const double apk = a[p][k], aqk = a[q][k];
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
                for (int k = 0; k < N; ++k) {
                    const double vkp = v[k][p], vkq = v[k][q];
                    v[k][p] = c * vkp - s * vkq;
                    v[k][q] = s * vkp + c * vkq;
                }
            }
    }
    for (int i = 0; i < N; ++i) w[i] = a[i][i];
}

static V3 Centroid(const std::vector<V3>& p) {
    V3 c{0, 0, 0};
    for (const auto& x : p)
        for (int i = 0; i < 3; ++i) c[i] += x[i];
    for (int i = 0; i < 3; ++i) c[i] /= double(p.size());
    return c;
}

// Principal axes as the columns of a rotation, largest spread first.
static M3 Axes(const std::vector<V3>& p, const V3& c) {
    std::array<std::array<double, 3>, 3> cov{};
    for (const auto& x : p)
        for (int i = 0; i < 3; ++i)
            for (int j = 0; j < 3; ++j) cov[i][j] += (x[i] - c[i]) * (x[j] - c[j]);
    std::array<double, 3> w{};
    std::array<std::array<double, 3>, 3> v{};
    Jacobi<3>(cov, w, v);
    std::array<int, 3> order{0, 1, 2};
    std::sort(order.begin(), order.end(), [&](int a, int b) { return w[a] > w[b]; });
    M3 m{};
    for (int col = 0; col < 3; ++col)
        for (int row = 0; row < 3; ++row) m[row][col] = v[row][order[col]];
    if (Det(m) < 0)
        for (int row = 0; row < 3; ++row) m[row][2] = -m[row][2];
    return m;
}

// The best rotation and translation for paired points (Horn's quaternion method): b ~= R a + t.
static void Horn(const std::vector<V3>& a, const std::vector<V3>& b, M3& R, V3& t) {
    const V3 ca = Centroid(a), cb = Centroid(b);
    double S[3][3] = {};
    for (size_t n = 0; n < a.size(); ++n)
        for (int i = 0; i < 3; ++i)
            for (int j = 0; j < 3; ++j) S[i][j] += (a[n][i] - ca[i]) * (b[n][j] - cb[j]);
    std::array<std::array<double, 4>, 4> N{};
    N[0][0] = S[0][0] + S[1][1] + S[2][2];
    N[0][1] = N[1][0] = S[1][2] - S[2][1];
    N[0][2] = N[2][0] = S[2][0] - S[0][2];
    N[0][3] = N[3][0] = S[0][1] - S[1][0];
    N[1][1] = S[0][0] - S[1][1] - S[2][2];
    N[1][2] = N[2][1] = S[0][1] + S[1][0];
    N[1][3] = N[3][1] = S[2][0] + S[0][2];
    N[2][2] = -S[0][0] + S[1][1] - S[2][2];
    N[2][3] = N[3][2] = S[1][2] + S[2][1];
    N[3][3] = -S[0][0] - S[1][1] + S[2][2];
    std::array<double, 4> w{};
    std::array<std::array<double, 4>, 4> v{};
    Jacobi<4>(N, w, v);
    int best = 0;
    for (int i = 1; i < 4; ++i)
        if (w[i] > w[best]) best = i;
    const double q0 = v[0][best], qx = v[1][best], qy = v[2][best], qz = v[3][best];
    R = {{{q0 * q0 + qx * qx - qy * qy - qz * qz, 2 * (qx * qy - q0 * qz), 2 * (qx * qz + q0 * qy)},
          {2 * (qy * qx + q0 * qz), q0 * q0 - qx * qx + qy * qy - qz * qz, 2 * (qy * qz - q0 * qx)},
          {2 * (qz * qx - q0 * qy), 2 * (qz * qy + q0 * qx), q0 * q0 - qx * qx - qy * qy + qz * qz}}};
    const V3 rc = Mul(R, ca);
    t = {cb[0] - rc[0], cb[1] - rc[1], cb[2] - rc[2]};
}

// Nearest target point through a uniform grid.
struct Grid {
    double h;
    std::unordered_map<long long, std::vector<int>> cells;
    const std::vector<V3>* pts;
    static long long Key(long long x, long long y, long long z) {
        return ((x + 1048576) << 42) | ((y + 1048576) << 21) | (z + 1048576);
    }
    Grid(const std::vector<V3>& p, double cell) : h(cell), pts(&p) {
        for (int i = 0; i < int(p.size()); ++i)
            cells[Key(llround(std::floor(p[i][0] / h)), llround(std::floor(p[i][1] / h)), llround(std::floor(p[i][2] / h)))]
                .push_back(i);
    }
    // Index and squared distance of the nearest point within `maxRing` cells, or -1.
    int Nearest(const V3& q, double& d2, int maxRing) const {
        const long long cx = llround(std::floor(q[0] / h)), cy = llround(std::floor(q[1] / h)), cz = llround(std::floor(q[2] / h));
        int best = -1;
        d2 = 1e300;
        for (int r = 0; r <= maxRing; ++r) {
            for (long long x = cx - r; x <= cx + r; ++x)
                for (long long y = cy - r; y <= cy + r; ++y)
                    for (long long z = cz - r; z <= cz + r; ++z) {
                        if (std::max({std::llabs(x - cx), std::llabs(y - cy), std::llabs(z - cz)}) != r) continue;
                        auto it = cells.find(Key(x, y, z));
                        if (it == cells.end()) continue;
                        for (int i : it->second) {
                            const V3& p = (*pts)[i];
                            const double dx = p[0] - q[0], dy = p[1] - q[1], dz = p[2] - q[2];
                            const double d = dx * dx + dy * dy + dz * dz;
                            if (d < d2) { d2 = d; best = i; }
                        }
                    }
            // a point found in ring r can still lose to one in ring r + 1 only if it is farther than r cells
            if (best >= 0 && std::sqrt(d2) <= r * h) break;
        }
        return best;
    }
};

// Nearest target point by trying them all (the coarse rounds, on a thinned target).
struct Brute {
    const std::vector<V3>* pts;
    explicit Brute(const std::vector<V3>& p) : pts(&p) {}
    int Nearest(const V3& q, double& d2, int) const {
        int best = -1;
        d2 = 1e300;
        for (int i = 0; i < int(pts->size()); ++i) {
            const V3& p = (*pts)[i];
            const double dx = p[0] - q[0], dy = p[1] - q[1], dz = p[2] - q[2];
            const double d = dx * dx + dy * dy + dz * dz;
            if (d < d2) { d2 = d; best = i; }
        }
        return best;
    }
};

struct Fit {
    M3 R;
    V3 t;
    double rms = 1e300;
    double near2mm = 0;
};

template <class NN>
static Fit Icp(const std::vector<V3>& src, const NN& nn, const std::vector<V3>& dst, M3 R, V3 t, double trim, int rings,
               int iters) {
    Fit f;
    std::vector<std::pair<double, int>> pairs;
    std::vector<int> match(src.size());
    double last = 1e300;
    for (int iter = 0; iter < iters; ++iter) {
        pairs.clear();
        for (int i = 0; i < int(src.size()); ++i) {
            V3 p = Mul(R, src[i]);
            for (int k = 0; k < 3; ++k) p[k] += t[k];
            double d2;
            const int j = nn.Nearest(p, d2, rings);
            if (j < 0) continue;
            match[i] = j;
            pairs.push_back({d2, i});
        }
        if (pairs.size() < 16) break;
        std::sort(pairs.begin(), pairs.end());
        const size_t keep = std::max<size_t>(16, size_t(pairs.size() * (1.0 - trim)));
        std::vector<V3> a, b;
        double sum = 0;
        for (size_t n = 0; n < keep; ++n) {
            a.push_back(src[pairs[n].second]);
            b.push_back(dst[match[pairs[n].second]]);
            sum += pairs[n].first;
        }
        const double rms = std::sqrt(sum / double(keep));
        size_t close = 0;
        for (const auto& pr : pairs)
            if (pr.first < 0.002 * 0.002) ++close;
        f.R = R;
        f.t = t;
        f.rms = rms;
        f.near2mm = double(close) / double(src.size());
        if (std::fabs(last - rms) < 1e-9) break;
        last = rms;
        Horn(a, b, R, t);
    }
    return f;
}

int main(int argc, char** argv) {
    if (argc < 3) {
        std::fprintf(stderr, "usage: shellfit SOURCE.bin TARGET.bin [TRIM]\n");
        return 1;
    }
    const double trim = argc > 3 ? std::atof(argv[3]) : 0.15;
    std::vector<V3> src = Load(argv[1]), dst = Load(argv[2]);
    if (src.size() < 16 || dst.size() < 16) {
        std::fprintf(stderr, "too few points\n");
        return 2;
    }
    // Coarse rounds on thinned clouds (every pair tried), from each start; then the winner refined over every source
    // point against the whole target through a grid (pairs farther than ~1.5 cm are left out there).
    auto thin = [](const std::vector<V3>& p, size_t n) {
        std::vector<V3> out;
        const size_t stride = std::max<size_t>(1, p.size() / n);
        for (size_t i = 0; i < p.size(); i += stride) out.push_back(p[i]);
        return out;
    };
    const std::vector<V3> srcThin = thin(src, 700), dstThin = thin(dst, 3500);
    const Brute brute(dstThin);
    const Grid grid(dst, 0.004);
    const V3 cs = Centroid(src), cd = Centroid(dst);
    const M3 As = Axes(src, cs), Ad = Axes(dst, cd);
    Fit best;
    const int signs[4][3] = {{1, 1, 1}, {1, -1, -1}, {-1, 1, -1}, {-1, -1, 1}};
    for (const auto& s : signs) {
        M3 D{};
        for (int i = 0; i < 3; ++i) D[i][i] = s[i];
        const M3 R0 = MulM(MulM(Ad, D), T(As));
        const V3 rc = Mul(R0, cs);
        const V3 t0 = {cd[0] - rc[0], cd[1] - rc[1], cd[2] - rc[2]};
        const Fit f = Icp(srcThin, brute, dstThin, R0, t0, trim, 0, 80);
        std::fprintf(stderr, "start %+d%+d%+d: rms %.5f\n", s[0], s[1], s[2], f.rms);
        if (f.rms < best.rms) best = f;
    }
    best = Icp(src, grid, dst, best.R, best.t, trim, 3, 40);
    const M3& R = best.R;
    // rotation matrix -> quaternion { i, j, k, r }
    double qi, qj, qk, qr;
    const double tr = R[0][0] + R[1][1] + R[2][2];
    if (tr > 0) {
        const double s = std::sqrt(tr + 1.0) * 2;
        qr = 0.25 * s; qi = (R[2][1] - R[1][2]) / s; qj = (R[0][2] - R[2][0]) / s; qk = (R[1][0] - R[0][1]) / s;
    } else if (R[0][0] > R[1][1] && R[0][0] > R[2][2]) {
        const double s = std::sqrt(1.0 + R[0][0] - R[1][1] - R[2][2]) * 2;
        qr = (R[2][1] - R[1][2]) / s; qi = 0.25 * s; qj = (R[0][1] + R[1][0]) / s; qk = (R[0][2] + R[2][0]) / s;
    } else if (R[1][1] > R[2][2]) {
        const double s = std::sqrt(1.0 + R[1][1] - R[0][0] - R[2][2]) * 2;
        qr = (R[0][2] - R[2][0]) / s; qi = (R[0][1] + R[1][0]) / s; qj = 0.25 * s; qk = (R[1][2] + R[2][1]) / s;
    } else {
        const double s = std::sqrt(1.0 + R[2][2] - R[0][0] - R[1][1]) * 2;
        qr = (R[1][0] - R[0][1]) / s; qi = (R[0][2] + R[2][0]) / s; qj = (R[1][2] + R[2][1]) / s; qk = 0.25 * s;
    }
    std::printf("{\"q\": [%.7f, %.7f, %.7f, %.7f], \"t\": [%.7f, %.7f, %.7f], \"rms\": %.6f, \"near2mm\": %.4f, "
                "\"source\": %zu, \"target\": %zu}\n",
                qi, qj, qk, qr, best.t[0], best.t[1], best.t[2], best.rms, best.near2mm, src.size(), dst.size());
    return 0;
}
