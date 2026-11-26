#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Utils/DebugGate.hpp"
// ViewConstants -- what each view's shaders are actually TOLD, as opposed to what we believe we set.
//
// Two probes with one purpose. cbv_probe_note records how many constant-buffer views each node binds
// per view, so "the second view's lighting is wrong" can be narrowed to a node that binds a different
// number of them. camcb_note goes further and reads the camera constant buffer itself -- position,
// basis, and three rows of the projection -- then PAIRS the two views' uploads within a few
// milliseconds of each other and prints them side by side.
//
// THE PAIRING IS THE WHOLE POINT. One view's constants read in isolation look plausible almost
// always; two views' constants read NEXT TO EACH OTHER show which field the second eye is missing.
// That is how the atmosphere block, the cloud wind offsets and the projection rows were each found,
// and it is why this file exists instead of a breakpoint.
//
// Every read goes through camcb_read_guarded, because the pointer comes from an upload heap the engine
// may already have recycled: a probe that crashes the game teaches nothing.

#include "Stereo/SyncStereo.hpp"
#include "Utils/StereoLog.hpp"
#include "Stereo/VrcamConfig.hpp"   // vrcam.json access + CName hashing, shared with the launcher
#include "Render/ColorBlit.hpp"   // HUD debug overlay on the mirror image
#include <windows.h>
#include <d3d12.h>
#include <d3d11.h>
#include <d3d11on12.h>
#include <dxgi1_4.h>
#include <intrin.h>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <thread>
#include <unordered_map>
#include <vector>
#include "MinHook.h"
#include "Utils/LogThrottle.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"

#if CVR_RUST_VRIK
// THE PROBES ARE RUST'S (rust/crates/cvr-render/src/view_constants.rs, called from the CBV hook by capture_cbv.rs): their
// exported switches and counters stay here, with the upload-heap lookup the C++ capture shares.
// OFF -- the 256-byte b6 blocks turned out to be ring garbage past register 0.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
extern "C" __declspec(dllexport) int32_t CyberpunkVR_CbvProbe = 1;
#endif
// Resolve a GPU virtual address inside a mapped upload heap to its CPU bytes.
// The mapped heaps are Rust's (rust/crates/cvr-render/src/upload_maps.rs).
// For the Rust censuses (rust/crates/cvr-render/src/census_draws.rs).
// One node's block, both views, in full. Live-settable so the next candidate costs no rebuild.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CbvDumpNode = 0x40FC6C;  // LightChannelVolumes
#endif
// OFF: a mutex and an 848-byte memcpy on every constant-buffer view over 768 bytes.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
extern "C" __declspec(dllexport) int32_t CyberpunkVR_CamCbProbe = 1;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CamPairMaxMs = 40;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCamPairs = 0;
#endif
#else
namespace cvr {
namespace detail {

// ---- what constants does each view's lighting actually get? --------------------------------
// Everything else is now equal by measurement: the same nodes dispatch, the same light array,
// the same bindings. The one asymmetry left standing is the 256-byte block bound at b6, where
// the capture showed MAIN carrying six world-space entries and a count of 3 while VRCAM's was
// empty with count 0. It lives in the upload ring and is bound in place, so it is invisible to
// CopyBufferRegion -- but CreateConstantBufferView hands us its GPU address, and the ring is
// already mapped for the cloud constants, so the bytes can be read right here.
// OFF -- the 256-byte b6 blocks turned out to be ring garbage past register 0.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_CbvProbe = 1;
// Resolve a GPU virtual address inside a mapped upload heap to its CPU bytes.
const uint8_t* upload_cpu_for_va(uint64_t va, uint64_t need) {
    std::lock_guard<std::mutex> lk(g_upload_map_mtx);
    for (uint32_t i = 0; i < g_upload_map_n; ++i) {
        const MappedUpload& m = g_upload_maps[i];
        if (!m.ptr || !m.va) continue;
        if (va >= m.va && va + need <= m.va + m.size) return m.ptr + (va - m.va);
    }
    return nullptr;
}

struct CbvSeen { uint32_t node_rva; uint32_t count[2]; uint32_t hits[2]; };
static std::array<CbvSeen, 48> g_cbv_seen{};
static uint32_t g_cbv_seen_n = 0;
static std::mutex g_cbv_mtx;

// SEH-guarded: the ring is engine memory and the block may be recycled mid-read.
bool cbv_read_head(const uint8_t* p, float* xy, uint32_t* w) {
    __try {
        memcpy(xy, p, 8);
        float fw; memcpy(&fw, p + 12, 4);
        if (!(fw >= 0.0f && fw < 1024.0f)) return false;
        *w = static_cast<uint32_t>(fw);
        return xy[0] > 0.0f && xy[0] < 0.01f && xy[1] > 0.0f && xy[1] < 0.01f;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

void cbv_probe_note(uint32_t node_rva, uint32_t count, bool vrcam) {
    bool dump = false;
    {
        std::lock_guard<std::mutex> lk(g_cbv_mtx);
        uint32_t i = 0;
        for (; i < g_cbv_seen_n; ++i) if (g_cbv_seen[i].node_rva == node_rva) break;
        if (i == g_cbv_seen_n) {
            if (g_cbv_seen_n >= g_cbv_seen.size()) return;
            g_cbv_seen[g_cbv_seen_n++] = { node_rva, {0, 0}, {0, 0} };
        }
        const int v = vrcam ? 1 : 0;
        ++g_cbv_seen[i].hits[v];
        if (count > g_cbv_seen[i].count[v]) { g_cbv_seen[i].count[v] = count; dump = true; }
    }
    if (!dump) return;
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 10000) return;
    s_last = now;
    char line[1100];
    int u = 0;
    line[0] = 0;
    std::lock_guard<std::mutex> lk(g_cbv_mtx);
    for (uint32_t k = 0; k < g_cbv_seen_n && u < static_cast<int>(sizeof(line)) - 40; ++k) {
        const CbvSeen& c = g_cbv_seen[k];
        u += snprintf(line + u, sizeof(line) - u, "%X:max m%u/v%u (n %u/%u) ",
                      c.node_rva, c.count[0], c.count[1], c.hits[0], c.hits[1]);
    }
    log("[cbv] 256B view-constant blocks, peak count field per node: %s", line);
}

// One node's block, both views, in full. The peak-count table says WHERE the views disagree;
// this says HOW. Live-settable so the next candidate costs no rebuild.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CbvDumpNode = 0x40FC6C;  // LightChannelVolumes
static std::mutex g_cbvd_mtx;
static uint8_t g_cbvd[2][256];
static bool    g_cbvd_have[2] = {false, false};

void cbv_dump_note(const uint8_t* p, bool vrcam) {
    uint8_t tmp[256];
    if (!cloud_cb_raw_copy(tmp, p, 256)) return;
    bool both = false;
    {
        std::lock_guard<std::mutex> lk(g_cbvd_mtx);
        const int v = vrcam ? 1 : 0;
        if (g_cbvd_have[v]) return;                 // first block per view is enough
        memcpy(g_cbvd[v], tmp, 256);
        g_cbvd_have[v] = true;
        both = g_cbvd_have[0] && g_cbvd_have[1];
    }
    if (!both) return;
    for (int half = 0; half < 2; ++half) {
        char line[1400];
        int u = 0;
        line[0] = 0;
        for (int r = half * 8; r < half * 8 + 8; ++r) {
            float fm[4], fv[4];
            memcpy(fm, g_cbvd[0] + r * 16, 16);
            memcpy(fv, g_cbvd[1] + r * 16, 16);
            if (u < static_cast<int>(sizeof(line)) - 170)
                u += snprintf(line + u, sizeof(line) - u,
                              "[%d] M(%.5g %.5g %.5g %.5g) V(%.5g %.5g %.5g %.5g)  ", r,
                              fm[0], fm[1], fm[2], fm[3], fv[0], fv[1], fv[2], fv[3]);
        }
        log("[cbvdump] node %X regs %d-%d: %s", CyberpunkVR_CbvDumpNode,
            half * 8, half * 8 + 7, line);
    }
}

// OFF: a mutex and an 848-byte memcpy on every constant-buffer view over 768 bytes. The stage
// counters logged 114195 of those in ten seconds.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_CamCbProbe = 1;
// filled_cpu_for_va moved with the census; declared in Stereo/StereoInternal.hpp.

// SEH cannot share a frame with objects that unwind, and the reader below holds a lock and uses
// lambdas -- hence the split, same as filled_note_guarded and pso_stream_find.
static bool camcb_read_guarded(const uint8_t* cp, float* out, size_t bytes) {
    __try { memcpy(out, cp, bytes); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return true;
}

// Counters per stage: a probe that finds nothing must be able to say WHERE it stopped.
std::atomic<uint64_t> g_cc_big{0}, g_cc_cpu{0}, g_cc_basis{0}, g_cc_pos{0}, g_cc_res{0};

// PAIR DETECTION, not attribution. Two things came out of the first run and both are settled
// here rather than argued about:
//
//   * the VRCAM flag was 0 on every single accepted entry -- descriptor creation does not happen
//     inside node dispatch, so `t_vrcam_node_active` cannot say which view a constant buffer
//     belongs to. It is not consulted any more.
//   * a five-second window mixes frames, so head motion produced eight "distinct" cameras that
//     were really one camera at eight instants.
//
// What identifies the eye pair needs neither: two camera buffers built within one frame of each
// other, a plausible eye separation apart. Head motion between frames is millimetres and cannot
// counterfeit 65 mm; the eye separation is constant and cannot be confused with drift.
static float g_cc_prev_pos[3]{};
static float g_cc_prev_basis[9]{};
static float g_cc_prev_proj[9]{};       // rows 28, 29, 31 of the view-projection
static uint64_t g_cc_prev_tick = 0;
static bool g_cc_prev_valid = false;
static std::mutex g_camcb_mtx;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CamPairMaxMs = 40;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCamPairs = 0;

void camcb_stages() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 10000) return;
    s_last = now;
    log("[camcb] stages: big=%llu cpu=%llu basis=%llu pos=%llu accepted=%llu pairs=%llu",
        (unsigned long long)g_cc_big.load(), (unsigned long long)g_cc_cpu.load(),
        (unsigned long long)g_cc_basis.load(), (unsigned long long)g_cc_pos.load(),
        (unsigned long long)g_cc_res.load(),
        (unsigned long long)CyberpunkVR_DebugCamPairs);
}

// a = the earlier camera, b = the later one, both from the same frame.
// Each view's projection, reduced to the four numbers that can differ and be seen.
//
// The VP acts on camera-relative coordinates, so clip.x = dot(row28, ip) and w = dot(row31, ip).
// For a plain symmetric perspective row28 is right/tan(halfH) and has NO component along forward;
// a component along forward IS the frustum's horizontal off-centre, and an off-centre frustum in
// one eye and not the other shifts that whole eye's image sideways by a constant angle -- which
// is exactly the symptom, and unlike parallax it does not care about distance.
static void camcb_proj(const char* tag, const float* proj, const float* basis) {
    const float* r28 = proj + 0;
    const float* r29 = proj + 3;
    const float* fwd = basis + 3;
    const float sx = sqrtf(r28[0]*r28[0] + r28[1]*r28[1] + r28[2]*r28[2]);
    const float sy = sqrtf(r29[0]*r29[0] + r29[1]*r29[1] + r29[2]*r29[2]);
    const float cx = r28[0]*fwd[0] + r28[1]*fwd[1] + r28[2]*fwd[2];
    const float cy = r29[0]*fwd[0] + r29[1]*fwd[1] + r29[2]*fwd[2];
    log("[campair] %s proj: sx=%.6f sy=%.6f  hfov=%.4f vfov=%.4f deg  offCentre x=%+.6f y=%+.6f",
        tag, sx, sy,
        (sx > 1e-6f) ? (2.0f * atanf(1.0f / sx) * 57.29578f) : 0.0f,
        (sy > 1e-6f) ? (2.0f * atanf(1.0f / sy) * 57.29578f) : 0.0f,
        cx, cy);
}

static void camcb_pair(const float* ap, const float* ab, const float* bp, const float* bb,
                       const float* aproj, const float* bproj) {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 3000) return;
    s_last = now;
    const float d[3] = { bp[0] - ap[0], bp[1] - ap[1], bp[2] - ap[2] };
    // Decomposed in the FIRST camera's own basis: rows 40/41/42 are right / forward / up
    // (verified by right x forward = up on a live dump). A correct eye separation is
    // (+-IPD, 0, 0) here; anything in the forward or up column is the separation going somewhere
    // it should not, and that is a stereo bug independent of the sight.
    const float dr = d[0]*ab[0] + d[1]*ab[1] + d[2]*ab[2];
    const float df = d[0]*ab[3] + d[1]*ab[4] + d[2]*ab[5];
    const float du = d[0]*ab[6] + d[1]*ab[7] + d[2]*ab[8];
    log("[campair] A=(%.5f %.5f %.5f)  B=(%.5f %.5f %.5f)", ap[0], ap[1], ap[2], bp[0], bp[1], bp[2]);
    log("[campair] delta=(%.5f %.5f %.5f) |d|=%.4f m  ->  right=%+.4f forward=%+.4f up=%+.4f",
        d[0], d[1], d[2], sqrtf(d[0]*d[0]+d[1]*d[1]+d[2]*d[2]), dr, df, du);
    log("[campair] A right=(%+.6f %+.6f %+.6f) fwd=(%+.6f %+.6f %+.6f)",
        ab[0], ab[1], ab[2], ab[3], ab[4], ab[5]);
    auto ang = [](const float* x, const float* y) {
        float t = x[0]*y[0] + x[1]*y[1] + x[2]*y[2];
        if (t > 1.0f) t = 1.0f;
        if (t < -1.0f) t = -1.0f;
        return acosf(t) * 1000.0f;                     // milliradians
    };
    log("[campair] axis disagreement: right=%.3f forward=%.3f up=%.3f mrad",
        ang(ab + 0, bb + 0), ang(ab + 3, bb + 3), ang(ab + 6, bb + 6));
    camcb_proj("A", aproj, ab);
    camcb_proj("B", bproj, bb);
}

void camcb_note(const uint8_t* cp, bool vrcam) {
    g_cc_cpu.fetch_add(1, std::memory_order_relaxed);
    float r[53 * 4];
    if (!camcb_read_guarded(cp, r, sizeof(r))) { camcb_stages(); return; }
    const float* p36 = r + 36 * 4;
    const float* p37 = r + 37 * 4;
    const float* p47 = r + 47 * 4;
    const float* b0 = r + 40 * 4;
    const float* b1 = r + 41 * 4;
    const float* b2 = r + 42 * 4;

    // Fingerprint. Four independent structural facts, because ONE of them (an orthonormal 3x3 at
    // this offset) matched 8303 unrelated buffers -- object transforms and skinning palettes are
    // full of those, and a thousand of them also had a plausible size in row 47.
    auto unit = [](const float* v) {
        const float n = v[0]*v[0] + v[1]*v[1] + v[2]*v[2];
        return n > 0.98f && n < 1.02f;
    };
    const float d01 = b0[0]*b1[0] + b0[1]*b1[1] + b0[2]*b1[2];
    const float d02 = b0[0]*b2[0] + b0[1]*b2[1] + b0[2]*b2[2];
    const float d12 = b1[0]*b2[0] + b1[1]*b2[1] + b1[2]*b2[2];
    if (!unit(b0) || !unit(b1) || !unit(b2) ||
        fabsf(d01) > 0.02f || fabsf(d02) > 0.02f || fabsf(d12) > 0.02f) { camcb_stages(); return; }
    g_cc_basis.fetch_add(1, std::memory_order_relaxed);

    // Position AND rebase origin, and the fact that they coincide. The engine rebases the world
    // on the camera, so [36].xyz == [37].xyz by construction, with [36].w = 0 and [37].w = 1.
    // Dropping this clause -- on the theory that it was an artefact of a session-less capture --
    // is exactly what let a thousand strangers through.
    if (p37[3] != 1.0f || p36[3] != 0.0f) { camcb_stages(); return; }
    if (fabsf(p36[0] - p37[0]) > 1e-3f || fabsf(p36[1] - p37[1]) > 1e-3f ||
        fabsf(p36[2] - p37[2]) > 1e-3f) { camcb_stages(); return; }
    if (fabsf(p36[0]) + fabsf(p36[1]) + fabsf(p36[2]) < 1.0f) { camcb_stages(); return; }
    g_cc_pos.fetch_add(1, std::memory_order_relaxed);

    // The view's pixel size: whole numbers in range, with .zw exactly (0, 1).
    if (p47[2] != 0.0f || p47[3] != 1.0f) { camcb_stages(); return; }
    if (!(p47[0] >= 256.0f && p47[0] <= 16384.0f && p47[1] >= 256.0f && p47[1] <= 16384.0f) ||
        p47[0] != floorf(p47[0]) || p47[1] != floorf(p47[1])) { camcb_stages(); return; }
    g_cc_res.fetch_add(1, std::memory_order_relaxed);

    // Small auxiliary views (256x256 probes at a different altitude showed up in the first run)
    // are not eyes; requiring a full-size view keeps them out without naming them.
    if (p47[0] < 1024.0f || p47[1] < 1024.0f) { camcb_stages(); return; }

    float basis[9];
    memcpy(basis + 0, b0, 12);
    memcpy(basis + 3, b1, 12);
    memcpy(basis + 6, b2, 12);
    float proj[9];
    memcpy(proj + 0, r + 28 * 4, 12);
    memcpy(proj + 3, r + 29 * 4, 12);
    memcpy(proj + 6, r + 31 * 4, 12);
    float prevPos[3], prevBasis[9], prevProj[9];
    bool pair = false;
    {
        std::lock_guard<std::mutex> lk(g_camcb_mtx);
        const uint64_t now = GetTickCount64();
        if (g_cc_prev_valid && now - g_cc_prev_tick <= CyberpunkVR_CamPairMaxMs) {
            const float dx = p36[0] - g_cc_prev_pos[0];
            const float dy = p36[1] - g_cc_prev_pos[1];
            const float dz = p36[2] - g_cc_prev_pos[2];
            const float dd = sqrtf(dx*dx + dy*dy + dz*dz);
            if (dd > 0.02f && dd < 0.15f) {
                memcpy(prevPos, g_cc_prev_pos, sizeof(prevPos));
                memcpy(prevBasis, g_cc_prev_basis, sizeof(prevBasis));
                memcpy(prevProj, g_cc_prev_proj, sizeof(prevProj));
                pair = true;
                CVR_DIAGNOSTIC(++CyberpunkVR_DebugCamPairs);
            }
        }
        memcpy(g_cc_prev_pos, p36, 12);
        memcpy(g_cc_prev_basis, basis, sizeof(basis));
        memcpy(g_cc_prev_proj, proj, sizeof(proj));
        g_cc_prev_tick = now;
        g_cc_prev_valid = true;
    }
    if (pair) camcb_pair(prevPos, prevBasis, p36, basis, prevProj, proj);
    camcb_stages();
}

}  // namespace detail
}  // namespace cvr
#endif
#endif
