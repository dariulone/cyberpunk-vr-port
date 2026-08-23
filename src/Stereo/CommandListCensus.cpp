// CommandListCensus -- what each view actually asks the GPU to do, counted rather than guessed.
//
// Twenty-odd probes, each the same three pieces: a fixed-size table, a *_note() called from a
// command-list hook, and a *_report() drained once per interval. They answer questions no static
// reading of the frame graph can -- how many draws, dispatches, indirect executes, light writes and
// culling counters each view issues, what a bound target actually contains, whether a resource is a
// tile grid, which pipeline state a draw came from.
//
// THIS FILE IS THE REASON MOST OF THIS MODULE'S HARD BUGS WERE SOLVABLE. The second view's missing
// passes, the bright/dark alternation, the HUD's surface, the outline's blend mode -- each was found
// by counting what the two views did differently, not by reading code. So the probes are kept even
// when idle, and the cost of each is one branch on a flag that is normally zero.
//
// The command-list vtable hooks that CALL them live here too, for the same reason a detour and its
// registration belong together: hk_DrawInstanced exists to feed draw_census_note and has no other
// purpose.
//
// WHAT DOES NOT MOVE, and it is the analogue of the problem the detour registry solved:
// patch_command_list_vtable stays in SyncStereo.cpp because it now installs hooks defined in THREE
// files -- these, the capture path's barrier and RTV hooks, and the DLSS band's viewport hooks. A
// vtable slot has no registry, so the patcher must name every hook it installs, which is exactly the
// coupling CVR_DETOUR removed for the RVA detours. Until a slot registry exists, these hooks are
// declared in Stereo/StereoInternal.hpp and the patcher reaches them from there.
//
// EVERY TABLE HERE IS FIXED-SIZE, and this project has paid four times for a fixed-size table that
// stops working without a word. Each *_report() prints its own count against its bound; a count
// sitting exactly at the bound is the signal that the answer is truncated, not that the number is
// stable.

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

namespace cvr {
namespace detail {

static void light_dst_note(void* dst, uint64_t size, uint32_t bytes, bool vrcam) {
    if (!dst || size > 4096) return;          // constant buffers only; the big arrays are noise
    uint32_t i = 0;
    for (; i < g_light_dst_n; ++i) if (g_light_dsts[i].res == dst) break;
    if (i == g_light_dst_n) {
        if (g_light_dst_n >= g_light_dsts.size()) return;
        g_light_dsts[g_light_dst_n++] = { dst, size, {0, 0}, bytes };
    }
    ++g_light_dsts[i].hits[vrcam ? 1 : 0];
    g_light_dsts[i].last_bytes = bytes;
}

struct LightSizeBin { uint32_t bytes; uint32_t hits[2]; uint64_t dst_size[2]; };
static std::array<LightSizeBin, 48> g_light_bins{};
static uint32_t g_light_bin_n = 0;
static std::mutex g_light_mtx;

static void light_census_note(uint64_t num_bytes, uint64_t dst_size, void* dst_res,
                              bool vrcam) {
    const uint32_t sz = static_cast<uint32_t>(num_bytes);
    std::lock_guard<std::mutex> lk(g_light_mtx);
    uint32_t i = 0;
    for (; i < g_light_bin_n; ++i) if (g_light_bins[i].bytes == sz) break;
    if (i == g_light_bin_n) {
        if (g_light_bin_n >= g_light_bins.size()) return;
        g_light_bins[g_light_bin_n++] = { sz, {0, 0}, {0, 0} };
    }
    ++g_light_bins[i].hits[vrcam ? 1 : 0];
    g_light_bins[i].dst_size[vrcam ? 1 : 0] = dst_size;
    light_dst_note(dst_res, dst_size, sz, vrcam);
}

// Reported as: upload size, how often each view makes it, and the destination buffer's own
// capacity. A size one view uploads and the other never does is exactly what we are hunting.
static void light_census_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 12000) return;
    LightSizeBin bins[48];
    uint32_t n;
    {
        std::lock_guard<std::mutex> lk(g_light_mtx);
        n = g_light_bin_n;
        for (uint32_t i = 0; i < n; ++i) bins[i] = g_light_bins[i];
    }
    bool both = false;
    for (uint32_t i = 0; i < n; ++i) if (bins[i].hits[1]) { both = true; break; }
    if (!n || !both) return;
    s_last = now;
    // Biggest uploads first -- the light array dwarfs the little constant blocks.
    for (uint32_t a = 0; a + 1 < n; ++a)
        for (uint32_t b = a + 1; b < n; ++b)
            if (bins[b].bytes > bins[a].bytes) { LightSizeBin t = bins[a]; bins[a] = bins[b]; bins[b] = t; }
    char line[1500];
    int used = 0;
    line[0] = '\0';
    for (uint32_t i = 0; i < n && used < static_cast<int>(sizeof(line)) - 48; ++i) {
        const char* flag = (bins[i].hits[0] && !bins[i].hits[1]) ? "!MAIN-only"
                         : (!bins[i].hits[0] && bins[i].hits[1]) ? "!VRCAM-only" : "";
        used += snprintf(line + used, sizeof(line) - used, "%uB m%u/v%u dst%llu%s | ",
                         bins[i].bytes, bins[i].hits[0], bins[i].hits[1],
                         (unsigned long long)(bins[i].dst_size[0] ? bins[i].dst_size[0]
                                                                  : bins[i].dst_size[1]),
                         flag);
    }
    log("[lights] upload sizes inside ClusteredLightsCull+RenderLightBuffers: %s", line);
    char dl[900];
    int du = 0;
    dl[0] = 0;
    {
        std::lock_guard<std::mutex> lk(g_light_mtx);
        for (uint32_t i = 0; i < g_light_dst_n && du < static_cast<int>(sizeof(dl)) - 64; ++i) {
            const LightDst& d = g_light_dsts[i];
            du += snprintf(dl + du, sizeof(dl) - du, "%p sz%llu m%u/v%u last%uB%s | ",
                           d.res, (unsigned long long)d.size, d.hits[0], d.hits[1],
                           d.last_bytes,
                           (d.hits[0] && !d.hits[1]) ? " !MAIN-only"
                         : (!d.hits[0] && d.hits[1]) ? " !VRCAM-only" : "");
        }
    }
    log("[lights] constant-buffer destinations (<=4KB): %s", dl);
}

// ---- name the node behind each compute dispatch -------------------------------------------
// The night capture pinned the defect to one constant block: the pass at PSO 926 gets six
// world-space light entries and a non-zero count for MAIN, and an empty list with count 0 for
// VRCAM. What the capture cannot say is WHICH frame-graph node issues that dispatch, and
// without the node there is nothing to read in the reverse dumps and nothing to hook.
//
// The dispatch is identifiable by shape: it writes the tile grid, so its group count is the
// render resolution over 16 in both axes. Recording the node RVA for every distinct
// (groupX, groupY) per view names it in one run -- and gives the same table for every other
// compute pass for free.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DispatchCensus = 1;   // OFF: [dispatch] per-node dispatch census
struct DispatchBin { uint32_t node_rva, gx, gy, gz; uint32_t hits[2]; };
static std::array<DispatchBin, 96> g_disp_bins{};
static uint32_t g_disp_bin_n = 0;
static std::mutex g_disp_mtx;

static void dispatch_census_note(uint32_t rva, UINT x, UINT y, UINT z, bool vrcam) {
    std::lock_guard<std::mutex> lk(g_disp_mtx);
    uint32_t i = 0;
    for (; i < g_disp_bin_n; ++i) {
        const DispatchBin& b = g_disp_bins[i];
        if (b.node_rva == rva && b.gx == x && b.gy == y && b.gz == z) break;
    }
    if (i == g_disp_bin_n) {
        if (g_disp_bin_n >= g_disp_bins.size()) return;
        g_disp_bins[g_disp_bin_n++] = { rva, x, y, z, {0, 0} };
    }
    ++g_disp_bins[i].hits[vrcam ? 1 : 0];
}

// Report every dispatch shape ONE view issues and the other never does, whatever its size.
// The first cut of this filtered to square tile grids and so only found passes that were
// already understood (the HUD surface and its blur pyramid, our own shadow reuse) -- the
// filter, not the engine, is what hid everything else.
static void dispatch_census_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 12000) return;
    // Both views have to have been seen at all, or every shape looks exclusive.
    bool anyv = false, anym = false;
    {
        std::lock_guard<std::mutex> lk(g_disp_mtx);
        for (uint32_t i = 0; i < g_disp_bin_n; ++i) {
            if (g_disp_bins[i].hits[0]) anym = true;
            if (g_disp_bins[i].hits[1]) anyv = true;
        }
    }
    if (!anym || !anyv) return;
    s_last = now;
    // Aggregate PER NODE, not per (node, shape). Thread-group counts are content-derived -- a
    // light or terrain-instance count -- so the two views almost never land on the same number,
    // and binning by the exact shape makes every such node look exclusive. That artefact is
    // what produced the earlier "ClusteredLightsCull 1964x1x1 is MAIN-only" reading: the
    // question is whether the node dispatches AT ALL for a view.
    struct NodeAgg { uint32_t rva; uint64_t hits[2]; uint32_t shapes[2]; };
    NodeAgg agg[96];
    uint32_t an = 0;
    {
        std::lock_guard<std::mutex> lk(g_disp_mtx);
        for (uint32_t i = 0; i < g_disp_bin_n; ++i) {
            const DispatchBin& b = g_disp_bins[i];
            uint32_t k = 0;
            for (; k < an; ++k) if (agg[k].rva == b.node_rva) break;
            if (k == an) { if (an >= 96) continue; agg[an++] = { b.node_rva, {0, 0}, {0, 0} }; }
            for (int s = 0; s < 2; ++s)
                if (b.hits[s]) { agg[k].hits[s] += b.hits[s]; ++agg[k].shapes[s]; }
        }
    }
    char mo[1200], vo[600];
    int mu = 0, vu = 0, mn = 0, vn = 0;
    mo[0] = 0; vo[0] = 0;
    for (uint32_t k = 0; k < an; ++k) {
        const NodeAgg& a = agg[k];
        if (a.hits[0] && !a.hits[1]) {
            ++mn;
            if (mu < static_cast<int>(sizeof(mo)) - 32)
                mu += snprintf(mo + mu, sizeof(mo) - mu, "%X(%llu) ", a.rva,
                               (unsigned long long)a.hits[0]);
        } else if (!a.hits[0] && a.hits[1]) {
            ++vn;
            if (vu < static_cast<int>(sizeof(vo)) - 32)
                vu += snprintf(vo + vu, sizeof(vo) - vu, "%X(%llu) ", a.rva,
                               (unsigned long long)a.hits[1]);
        }
    }
    log("[disp] nodes that dispatch for MAIN and NEVER for VRCAM (%d of %u): %s",
        mn, an, mo);
    log("[disp] nodes that dispatch for VRCAM and NEVER for MAIN (%d): %s", vn, vo);
}

// OFF -- the light arrays are byte-identical between the views.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_LightContent = 0;
constexpr uint64_t LIGHT_SNAP_MAX = 64 * 1024;
static std::mutex g_lc_mtx;
static uint8_t  g_lc_buf[2][LIGHT_SNAP_MAX];
static uint32_t g_lc_len[2] = {0, 0};

// The upload ring is CPU-visible and already mapped for the cloud constants, so the bytes the
// engine is about to copy can be read here directly -- no readback, no extra GPU work.
static void light_content_note(ID3D12Resource* src, uint64_t off, uint64_t n, bool vrcam) {
    const uint8_t* p = upload_map_read(src);
    if (!p) return;
    uint8_t tmp[LIGHT_SNAP_MAX];
    if (!cloud_cb_raw_copy(tmp, p + off, static_cast<size_t>(n))) return;
    const int i = vrcam ? 1 : 0;
    {
        std::lock_guard<std::mutex> lk(g_lc_mtx);
        if (n <= g_lc_len[i]) return;              // keep the largest seen: that is the array
        memcpy(g_lc_buf[i], tmp, static_cast<size_t>(n));
        g_lc_len[i] = static_cast<uint32_t>(n);
    }
}

// Compare as 16-byte records, which is how the entries in MAIN's block decoded in the capture
// (world position + a scalar). Report the lengths, how many records differ, and how many look
// like real world-space positions in each -- a zeroed tail shows up immediately.
static void light_content_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 12000) return;
    uint8_t a[LIGHT_SNAP_MAX], b[LIGHT_SNAP_MAX];
    uint32_t la, lb;
    {
        std::lock_guard<std::mutex> lk(g_lc_mtx);
        la = g_lc_len[0]; lb = g_lc_len[1];
        if (!la || !lb) return;
        memcpy(a, g_lc_buf[0], la); memcpy(b, g_lc_buf[1], lb);
    }
    s_last = now;
    auto worldish = [](const uint8_t* p, uint32_t len) {
        uint32_t c = 0;
        for (uint32_t o = 0; o + 12 <= len; o += 16) {
            float f[3]; memcpy(f, p + o, 12);
            if (fabsf(f[0]) > 100.0f && fabsf(f[1]) > 100.0f && fabsf(f[2]) < 500.0f) ++c;
        }
        return c;
    };
    const uint32_t common = la < lb ? la : lb;
    uint32_t diff = 0, first = 0xFFFFFFFF;
    for (uint32_t o = 0; o + 16 <= common; o += 16)
        if (memcmp(a + o, b + o, 16)) { ++diff; if (first == 0xFFFFFFFF) first = o; }
    log("[lightbuf] MAIN %u B (%u world-ish recs) | VRCAM %u B (%u) | of %u common recs %u differ,"
        " first at +%X", la, worldish(a, la), lb, worldish(b, lb), common / 16, diff,
        first == 0xFFFFFFFF ? 0 : first);
    // WHICH field differs decides everything. A lane that differs by ~0.064 is a world position
    // shifted by the IPD and is correct; a lane that differs wildly is view-derived data
    // computed wrong for VRCAM -- e.g. a screen-space bound or tile range, which would put the
    // lights in the wrong tiles and leave them unlit while the list itself looks perfect.
    double maxd[4] = {0, 0, 0, 0};
    uint32_t dcnt[4] = {0, 0, 0, 0};
    for (uint32_t o = 0; o + 16 <= common; o += 16) {
        for (int L = 0; L < 4; ++L) {
            float x, y;
            memcpy(&x, a + o + L * 4, 4);
            memcpy(&y, b + o + L * 4, 4);
            if (memcmp(a + o + L * 4, b + o + L * 4, 4)) {
                ++dcnt[L];
                const double d = fabs((double)x - (double)y);
                if (d > maxd[L] && d < 1e30) maxd[L] = d;
            }
        }
    }
    log("[lightbuf] per-lane: L0 %u diff max %.4g | L1 %u max %.4g | L2 %u max %.4g |"
        " L3 %u max %.4g", dcnt[0], maxd[0], dcnt[1], maxd[1], dcnt[2], maxd[2],
        dcnt[3], maxd[3]);
    char r0[700];
    int u0 = 0;
    r0[0] = 0;
    for (uint32_t k = 0; k < 5 && (k + 1) * 16 <= common; ++k) {
        float fa[4], fb[4];
        memcpy(fa, a + k * 16, 16);
        memcpy(fb, b + k * 16, 16);
        if (u0 < (int)sizeof(r0) - 160)
            u0 += snprintf(r0 + u0, sizeof(r0) - u0,
                           "[%u] M(%.4g %.4g %.4g %.4g) V(%.4g %.4g %.4g %.4g)  ", k,
                           fa[0], fa[1], fa[2], fa[3], fb[0], fb[1], fb[2], fb[3]);
    }
    log("[lightbuf] first records %s", r0);
}

// ---- is the two views' auto-exposure the same? ---------------------------------------------
// The lighting output has the lamps in BOTH views (checked in the capture), and every node after
// it -- ApplyBloomAndTonemapping, GenerateTonemappingLUT, HistogramUpdate -- runs symmetrically.
// What those share as an input but hold PER VIEW is the exposure: each view has its own 28-byte
// FrameExposureData. A higher exposure for VRCAM would compress the highlights and keep the
// lamps under the bloom threshold -- light present, glow absent, which is exactly the symptom.
// Read here rather than inferred: the buffer is copied into a readback in the engine's own list
// at the barrier that already identifies it, so no state is guessed and nothing is added to any
// queue of ours.
// OFF -- exposure differs by 10-35%: real, but recorded, not being re-measured.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ExpoProbe = 1;
extern "C" __declspec(dllexport) float   CyberpunkVR_DebugExpoValMain  = 0.f;
extern "C" __declspec(dllexport) float   CyberpunkVR_DebugExpoValVrcam = 0.f;
static ID3D12Resource* g_expo_rb[2] = {nullptr, nullptr};
static uint8_t*        g_expo_rb_map[2] = {nullptr, nullptr};

static bool expo_rb_ensure(int v) {
    if (g_expo_rb[v]) return g_expo_rb_map[v] != nullptr;
    if (!g_game_device) return false;
    D3D12_HEAP_PROPERTIES hp{};
    hp.Type = D3D12_HEAP_TYPE_READBACK;
    hp.CreationNodeMask = hp.VisibleNodeMask = 1;
    D3D12_RESOURCE_DESC bd{};
    bd.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    bd.Width = 256;
    bd.Height = 1; bd.DepthOrArraySize = 1; bd.MipLevels = 1;
    bd.Format = DXGI_FORMAT_UNKNOWN; bd.SampleDesc.Count = 1;
    bd.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ID3D12Resource* rb = nullptr;
    if (FAILED(g_game_device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &bd,
            D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&rb))) || !rb) return false;
    void* m = nullptr;
    D3D12_RANGE none{0, 0};
    if (FAILED(rb->Map(0, &none, &m)) || !m) { rb->Release(); return false; }
    rb->SetName(v ? L"CyberpunkVR_ExpoRbVrcam" : L"CyberpunkVR_ExpoRbMain");
    g_expo_rb[v] = rb;
    g_expo_rb_map[v] = static_cast<uint8_t*>(m);
    return true;
}

// ---- THE CLOUD BRIGHTNESS, and why it is only the clouds ------------------------------------
//
// Nsight settles it. The cloud raymarch is PipelineState_1301, an indirect dispatch in
// Transparents that read-writes the half-res cloud buffer, and its last line is:
//
//     float _471 = WaveReadLaneFirst(asfloat(_8.Load(0u).x));      // _8 = t37
//     _23[pixel] = float4(_471 * r, _471 * g, _471 * b, alpha);
//
// and t37 is bound to `StructuredBuffer<FrameExposureData>` -- Resource_30946 on the second view,
// Resource_2803 on MAIN. The clouds are multiplied by THAT VIEW'S EXPOSURE at raymarch time.
// Alpha is left alone.
//
// Everything else in the frame takes exposure later, at tonemap, where each view applies its own
// consistently. The clouds are the only thing that bakes it early, so a per-view exposure
// difference lands on the clouds and nowhere else -- a flat multiplier, not noise, which is
// exactly the reported "VRCAM's clouds are always lighter".
//
// And the difference was already measured by the probe below: 10-35%.
//
// Two eyes 6.5 cm apart have no business metering differently -- unequal brightness between eyes
// is binocular rivalry, tiring even when it is not consciously noticed. So this is a correction,
// not a workaround: hand the second view MAIN's exposure.
//
// Done with the probe's own proven mechanism. The buffer is copied at the barrier that hands it
// to the shaders, so its state is known exactly, and the copy goes through a private staging
// buffer rather than touching MAIN's resource from the second view's list: MAIN's copy is taken
// where MAIN's state is known, VRCAM's is written where VRCAM's is. Frame order is VRCAM-first,
// so the value applied is MAIN's from the previous frame -- exposure adapts over many frames, so
// one frame of lag is nothing.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_ExpoMirror = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugExpoMirrors = 0;
static ID3D12Resource* g_expo_stage = nullptr;

static bool expo_stage_ensure() {
    if (g_expo_stage) return true;
    if (!g_game_device) return false;
    D3D12_HEAP_PROPERTIES hp{};
    hp.Type = D3D12_HEAP_TYPE_DEFAULT;
    hp.CreationNodeMask = hp.VisibleNodeMask = 1;
    D3D12_RESOURCE_DESC bd{};
    bd.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    bd.Width = 256;
    bd.Height = 1; bd.DepthOrArraySize = 1; bd.MipLevels = 1;
    bd.Format = DXGI_FORMAT_UNKNOWN; bd.SampleDesc.Count = 1;
    bd.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ID3D12Resource* r = nullptr;
    if (FAILED(g_game_device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &bd,
            D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&r))) || !r) return false;
    r->SetName(L"CyberpunkVR_ExpoStage");
    g_expo_stage = r;
    return true;
}

 void expo_mirror(ID3D12GraphicsCommandList* list, ID3D12Resource* src, bool vrcam) {
    if (!CyberpunkVR_ExpoMirror || !list || !src) return;
    if (!expo_stage_ensure()) return;
    const CommandListVtableHook* e = command_list_hook_entry(list);
    if (!e || !e->barrier_call || !e->cbr_original) return;
    const D3D12_RESOURCE_STATES kSrv = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE |
                                       D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    D3D12_RESOURCE_BARRIER b{};
    b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition.pResource = src;
    b.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    if (!vrcam) {
        // MAIN: park a copy of its exposure in our staging buffer.
        b.Transition.StateBefore = kSrv;
        b.Transition.StateAfter  = D3D12_RESOURCE_STATE_COPY_SOURCE;
        e->barrier_call(list, 1, &b);
        e->cbr_original(list, g_expo_stage, 0, src, 0, 28);
        b.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
        b.Transition.StateAfter  = kSrv;
        e->barrier_call(list, 1, &b);
        return;
    }
    // VRCAM: overwrite its exposure with MAIN's.
    D3D12_RESOURCE_BARRIER s{};
    s.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    s.Transition.pResource = g_expo_stage;
    s.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    s.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    s.Transition.StateAfter  = D3D12_RESOURCE_STATE_COPY_SOURCE;
    e->barrier_call(list, 1, &s);
    b.Transition.StateBefore = kSrv;
    b.Transition.StateAfter  = D3D12_RESOURCE_STATE_COPY_DEST;
    e->barrier_call(list, 1, &b);
    e->cbr_original(list, src, 0, g_expo_stage, 0, 28);
    b.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    b.Transition.StateAfter  = kSrv;
    e->barrier_call(list, 1, &b);
    s.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    s.Transition.StateAfter  = D3D12_RESOURCE_STATE_COPY_DEST;
    e->barrier_call(list, 1, &s);
    InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugExpoMirrors));
}


// Appended to the engine's own list at the barrier that hands the buffer to the shaders, so its
// state is known exactly: it is going to *_SHADER_RESOURCE, which is where a copy source is legal.
 void expo_probe_copy(ID3D12GraphicsCommandList* list, ID3D12Resource* src, bool vrcam) {
    if (!CyberpunkVR_ExpoProbe || !list || !src) return;
    const int v = vrcam ? 1 : 0;
    if (!expo_rb_ensure(v)) return;
    const CommandListVtableHook* e = command_list_hook_entry(list);
    if (!e || !e->barrier_call || !e->cbr_original) return;
    D3D12_RESOURCE_BARRIER b{};
    b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition.pResource = src;
    b.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    b.Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE |
                               D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    b.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    e->barrier_call(list, 1, &b);
    e->cbr_original(list, g_expo_rb[v], 0, src, 0, 28);
    b.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    b.Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE |
                              D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    e->barrier_call(list, 1, &b);
}

 void expo_probe_report() {
    static uint64_t s_last = 0;
    if (!g_expo_rb_map[0] || !g_expo_rb_map[1]) return;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 8000) return;
    s_last = now;
    float m[7], v[7];
    memcpy(m, g_expo_rb_map[0], 28);
    memcpy(v, g_expo_rb_map[1], 28);
    CyberpunkVR_DebugExpoValMain  = m[6];
    CyberpunkVR_DebugExpoValVrcam = v[6];
    log("[expo] MAIN %.6g %.6g %.6g %.6g %.6g %.6g %.6g | VRCAM %.6g %.6g %.6g %.6g %.6g %.6g %.6g",
        m[0], m[1], m[2], m[3], m[4], m[5], m[6],
        v[0], v[1], v[2], v[3], v[4], v[5], v[6]);
}

// ---- does the light cull actually FILL the tile grid for VRCAM? ----------------------------
// The lamp's own emissive surface shows in both views; the light it casts does not -- no red
// wash under the red lamps, no lit roof, no lit road. In a clustered renderer that is exactly
// "the light list is fine but the per-tile assignment is empty", and the list IS fine: compared
// byte for byte, same length, same entries. So read the cull's OUTPUT -- the R16_UINT grid at
// render-res/32 that the lighting pass indexes. Non-zero tiles for MAIN and zeros for VRCAM
// would settle this; equal grids move the search to the lighting shader's other inputs.
// OFF -- the per-tile grids read equal once the readback was finally synchronised.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_TileProbe = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugTileNonzeroMain  = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugTileNonzeroVrcam = 0;

// One signature matches several different integer grids, and the first version silently mixed
// them: MAIN reported a stable 160x160 (max 5, i.e. a per-tile light COUNT) while VRCAM reported
// a 93x160 with wildly different statistics -- a different resource, not a different result.
// So keep every distinct grid separately, keyed by its dimensions, and report them all. Only
// grids of the SAME shape are comparable between the views.
// A single readback buffer read on a timer is worthless: the copy is recorded into the ENGINE'S
// list and nothing waits for the GPU, so the report shows a half-written or stale grid -- which
// is how MAIN's grid read full in one run and empty in the next while the symptom never moved.
// Three buffers, written round-robin, and only the OLDEST is read: by then two more copies have
// been recorded behind it, so the one being read is long since retired.
struct TileGrid {
    uint32_t w, h, pitch;
    DXGI_FORMAT fmt;
    ID3D12Resource* rb[3];
    uint8_t* map[3];
    uint32_t writes;
    uint64_t nz, mx, sum;
    bool seen;
};
static std::array<TileGrid, 8> g_tiles[2]{};
static uint32_t g_tile_n[2] = {0, 0};

 bool tile_is_grid(const D3D12_RESOURCE_DESC& d) {
    if (d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D) return false;
    if (d.MipLevels != 1 || d.DepthOrArraySize != 1) return false;
    if (!(d.Flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS)) return false;
    if (d.Width < 40 || d.Width > 220 || d.Height < 40 || d.Height > 220) return false;
    return d.Format == DXGI_FORMAT_R16_UINT || d.Format == DXGI_FORMAT_R32_UINT ||
           d.Format == DXGI_FORMAT_R8_UINT  || d.Format == DXGI_FORMAT_R32G32_UINT;
}

static TileGrid* tile_slot(int v, const D3D12_RESOURCE_DESC& d) {
    const uint32_t w = static_cast<uint32_t>(d.Width);
    for (uint32_t i = 0; i < g_tile_n[v]; ++i) {
        TileGrid& t = g_tiles[v][i];
        if (t.w == w && t.h == d.Height && t.fmt == d.Format) return &t;
    }
    if (g_tile_n[v] >= g_tiles[v].size() || !g_game_device) return nullptr;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT fp{};
    UINT64 total = 0;
    g_game_device->GetCopyableFootprints(&d, 0, 1, 0, &fp, nullptr, nullptr, &total);
    D3D12_HEAP_PROPERTIES hp{};
    hp.Type = D3D12_HEAP_TYPE_READBACK;
    hp.CreationNodeMask = 1;
    hp.VisibleNodeMask = 1;
    D3D12_RESOURCE_DESC bd{};
    bd.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    bd.Width = total;
    bd.Height = 1;
    bd.DepthOrArraySize = 1;
    bd.MipLevels = 1;
    bd.Format = DXGI_FORMAT_UNKNOWN;
    bd.SampleDesc.Count = 1;
    bd.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ID3D12Resource* rb[3] = {nullptr, nullptr, nullptr};
    uint8_t* mp[3] = {nullptr, nullptr, nullptr};
    D3D12_RANGE none{0, 0};
    for (int k = 0; k < 3; ++k) {
        if (FAILED(g_game_device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &bd,
                D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&rb[k]))) || !rb[k]) {
            for (int j = 0; j < k; ++j) rb[j]->Release();
            return nullptr;
        }
        void* m = nullptr;
        if (FAILED(rb[k]->Map(0, &none, &m)) || !m) {
            for (int j = 0; j <= k; ++j) rb[j]->Release();
            return nullptr;
        }
        mp[k] = static_cast<uint8_t*>(m);
    }
    TileGrid& t = g_tiles[v][g_tile_n[v]++];
    t.w = w; t.h = d.Height; t.pitch = fp.Footprint.RowPitch; t.fmt = d.Format;
    for (int k = 0; k < 3; ++k) { t.rb[k] = rb[k]; t.map[k] = mp[k]; }
    t.writes = 0;
    t.nz = t.mx = t.sum = 0; t.seen = false;
    return &t;
}

// Appended at the barrier that publishes the grid, so the source state is exact rather than
// assumed -- the same discipline as every other inline copy in this file.
 void tile_probe_copy(ID3D12GraphicsCommandList* list, ID3D12Resource* src,
                            const D3D12_RESOURCE_DESC& d, D3D12_RESOURCE_STATES after,
                            bool vrcam) {
    if (!CyberpunkVR_TileProbe || !list || !src || !g_game_device) return;
    const int v = vrcam ? 1 : 0;
    TileGrid* t = tile_slot(v, d);
    if (!t) return;
    const CommandListVtableHook* e = command_list_hook_entry(list);
    if (!e || !e->barrier_call || !e->copytex) return;
    D3D12_RESOURCE_BARRIER b{};
    b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition.pResource = src;
    b.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    b.Transition.StateBefore = after;
    b.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    e->barrier_call(list, 1, &b);
    D3D12_TEXTURE_COPY_LOCATION dl{}, sl{};
    dl.pResource = t->rb[t->writes % 3];
    dl.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    dl.PlacedFootprint.Offset = 0;
    dl.PlacedFootprint.Footprint.Format = d.Format;
    dl.PlacedFootprint.Footprint.Width = t->w;
    dl.PlacedFootprint.Footprint.Height = t->h;
    dl.PlacedFootprint.Footprint.Depth = 1;
    dl.PlacedFootprint.Footprint.RowPitch = t->pitch;
    sl.pResource = src;
    sl.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    sl.SubresourceIndex = 0;
    e->copytex(list, &dl, 0, 0, 0, &sl, nullptr);
    b.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    b.Transition.StateAfter = after;
    e->barrier_call(list, 1, &b);
    ++t->writes;
    t->seen = true;
}

static void tile_scan(TileGrid& t) {
    // writes-3 is the oldest of the three, i.e. two further copies were recorded after it.
    const uint8_t* base = t.map[(t.writes + 0) % 3];
    const uint32_t stride = (t.fmt == DXGI_FORMAT_R8_UINT)     ? 1u
                          : (t.fmt == DXGI_FORMAT_R16_UINT)    ? 2u
                          : (t.fmt == DXGI_FORMAT_R32G32_UINT) ? 8u : 4u;
    t.nz = t.mx = t.sum = 0;
    for (uint32_t y = 0; y < t.h; ++y) {
        const uint8_t* row = base + static_cast<size_t>(y) * t.pitch;
        for (uint32_t x = 0; x < t.w; ++x) {
            uint64_t val = 0;
            memcpy(&val, row + static_cast<size_t>(x) * stride, stride > 8 ? 8 : stride);
            if (val) { ++t.nz; t.sum += val; if (val > t.mx) t.mx = val; }
        }
    }
}

 void tile_probe_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 8000) return;
    if (!g_tile_n[0] || !g_tile_n[1]) return;
    s_last = now;
    for (int v = 0; v < 2; ++v) {
        char line[900];
        int u = 0;
        line[0] = 0;
        for (uint32_t i = 0; i < g_tile_n[v]; ++i) {
            TileGrid& t = g_tiles[v][i];
            if (!t.seen || t.writes < 3) continue;   // ring not filled yet: nothing retired
            tile_scan(t);
            if (u < static_cast<int>(sizeof(line)) - 80)
                u += snprintf(line + u, sizeof(line) - u,
                              "%ux%u fmt%u nz%llu max%llu sum%llu | ", t.w, t.h,
                              static_cast<unsigned>(t.fmt), (unsigned long long)t.nz,
                              (unsigned long long)t.mx, (unsigned long long)t.sum);
            if (v == 0) CyberpunkVR_DebugTileNonzeroMain = t.nz;
            else        CyberpunkVR_DebugTileNonzeroVrcam = t.nz;
        }
        log("[tile] %-5s grids: %s", v ? "VRCAM" : "MAIN", u ? line : "(none)");
    }
}

// ---- how many lights does each view's cull actually output? --------------------------------
// The capture shows both views clearing and filling the SAME persistent UAV buffers for the
// clustered light list -- Resource_1360 (20 B counter), 2980/2981 (3 MB each), 2991, 2994, 4943 --
// while some neighbouring counters are per-view transients. A 20-byte counter is the cheapest
// thing in that set to read, and it separates the two remaining explanations outright:
//   count == 0 for VRCAM  -> its cull produces nothing, and the fault is in the cull's inputs;
//   count  > 0 for VRCAM  -> the cull works and the result is overwritten before its lighting
//                            reads it, i.e. the shared buffers are raced (AsyncComputeDuring-
//                            Shadowmaps is a separate list and a likely second writer).
// Read off a UAV barrier, not a transition: this counter never changes state, it stays in
// UNORDERED_ACCESS, so the copy brackets it explicitly from a state we know rather than guess.
// OFF -- the 20-byte counters it found were indirect-draw args, now understood.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_CullCountProbe = 1;
struct CullCnt {
    ID3D12Resource* res;
    ID3D12Resource* rb[3];
    uint8_t*        map[3];
    uint32_t        writes;
    uint32_t        bytes;
    uint32_t        last[2][5];
    bool            seen[2];
};
static std::array<CullCnt, 6> g_cull{};
static uint32_t g_cull_n = 0;

static CullCnt* cull_slot(ID3D12Resource* res, uint32_t bytes) {
    for (uint32_t i = 0; i < g_cull_n; ++i) if (g_cull[i].res == res) return &g_cull[i];
    if (g_cull_n >= g_cull.size() || !g_game_device) return nullptr;
    D3D12_HEAP_PROPERTIES hp{};
    hp.Type = D3D12_HEAP_TYPE_READBACK;
    hp.CreationNodeMask = 1;
    hp.VisibleNodeMask = 1;
    D3D12_RESOURCE_DESC bd{};
    bd.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    bd.Width = 256;
    bd.Height = 1;
    bd.DepthOrArraySize = 1;
    bd.MipLevels = 1;
    bd.Format = DXGI_FORMAT_UNKNOWN;
    bd.SampleDesc.Count = 1;
    bd.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    CullCnt& c = g_cull[g_cull_n];
    D3D12_RANGE none{0, 0};
    for (int k = 0; k < 3; ++k) {
        ID3D12Resource* rb = nullptr;
        if (FAILED(g_game_device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &bd,
                D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&rb))) || !rb) {
            for (int j = 0; j < k; ++j) c.rb[j]->Release();
            return nullptr;
        }
        void* m = nullptr;
        if (FAILED(rb->Map(0, &none, &m)) || !m) {
            rb->Release();
            for (int j = 0; j < k; ++j) c.rb[j]->Release();
            return nullptr;
        }
        c.rb[k] = rb;
        c.map[k] = static_cast<uint8_t*>(m);
    }
    c.res = res;
    c.bytes = bytes > 20 ? 20 : bytes;
    c.writes = 0;
    memset(c.last, 0, sizeof(c.last));
    c.seen[0] = c.seen[1] = false;
    ++g_cull_n;
    return &c;
}

 void cull_count_note(ID3D12GraphicsCommandList* list, ID3D12Resource* res,
                            uint32_t bytes, bool vrcam) {
    if (!CyberpunkVR_CullCountProbe || !list || !res) return;
    CullCnt* c = cull_slot(res, bytes);
    if (!c) return;
    const CommandListVtableHook* e = command_list_hook_entry(list);
    if (!e || !e->barrier_call || !e->cbr_original) return;
    D3D12_RESOURCE_BARRIER b{};
    b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition.pResource = res;
    b.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    b.Transition.StateBefore = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
    b.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    e->barrier_call(list, 1, &b);
    e->cbr_original(list, c->rb[c->writes % 3], 0, res, 0, c->bytes);
    b.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    b.Transition.StateAfter = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
    e->barrier_call(list, 1, &b);
    ++c->writes;
    c->seen[vrcam ? 1 : 0] = true;
    // Snapshot into the per-view row from the OLDEST ring entry, which the GPU has long retired.
    if (c->writes >= 3) {
        const uint8_t* oldest = c->map[c->writes % 3];
        memcpy(c->last[vrcam ? 1 : 0], oldest, c->bytes);
    }
}

 void cull_count_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 8000) return;
    bool both = false;
    for (uint32_t i = 0; i < g_cull_n; ++i) if (g_cull[i].seen[0] && g_cull[i].seen[1]) both = true;
    if (!g_cull_n) return;
    s_last = now;
    char line[1000];
    int u = 0;
    line[0] = 0;
    for (uint32_t i = 0; i < g_cull_n && u < static_cast<int>(sizeof(line)) - 90; ++i) {
        const CullCnt& c = g_cull[i];
        u += snprintf(line + u, sizeof(line) - u,
                      "%p(%uB,%s%s) M[%u %u %u %u %u] V[%u %u %u %u %u] | ", c.res, c.bytes,
                      c.seen[0] ? "m" : "-", c.seen[1] ? "v" : "-",
                      c.last[0][0], c.last[0][1], c.last[0][2], c.last[0][3], c.last[0][4],
                      c.last[1][0], c.last[1][1], c.last[1][2], c.last[1][3], c.last[1][4]);
    }
    log("[cull] small UAV counters (shared ones carry both m and v): %s%s", line,
        both ? "" : "  [no counter seen by both views yet]");
}

// ---- the work the Dispatch census could never see -------------------------------------------
// The lighting is issued through ExecuteIndirect, which is vtable slot 59 -- an entirely
// different call from Dispatch (slot 14). Every census so far was blind to it, which is why they
// all reported the lighting nodes as symmetric while the light was plainly missing.
//
// The capture says what to expect: inside the Lighting list MAIN issues one
// CommandSignature_81 / Resource_1359 indirect DRAW (that signature is used 126 further times in
// GBuffer_Discard and the shadow cascades, so it is a draw, not a dispatch -- i.e. light volumes)
// and VRCAM issues none, while every other signature/argument pair matches one for one.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_IndirectCensus = 1;   // OFF: [indirect] ExecuteIndirect census
struct IndirectBin { uint32_t node_rva; void* sig; uint64_t hits[2]; };
static std::array<IndirectBin, 64> g_ind{};
static uint32_t g_ind_n = 0;
static std::mutex g_ind_mtx;

static void indirect_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 10000) return;
    IndirectBin b[64];
    uint32_t n;
    {
        std::lock_guard<std::mutex> lk(g_ind_mtx);
        n = g_ind_n;
        for (uint32_t i = 0; i < n; ++i) b[i] = g_ind[i];
    }
    bool anym = false, anyv = false;
    for (uint32_t i = 0; i < n; ++i) { if (b[i].hits[0]) anym = true; if (b[i].hits[1]) anyv = true; }
    if (!anym || !anyv) return;
    s_last = now;
    char mo[900], vo[900];
    int mu = 0, vu = 0;
    mo[0] = 0; vo[0] = 0;
    for (uint32_t i = 0; i < n; ++i) {
        if (b[i].hits[0] && !b[i].hits[1] && mu < static_cast<int>(sizeof(mo)) - 48)
            mu += snprintf(mo + mu, sizeof(mo) - mu, "%X/sig%p(%llu) ", b[i].node_rva, b[i].sig,
                           (unsigned long long)b[i].hits[0]);
        if (!b[i].hits[0] && b[i].hits[1] && vu < static_cast<int>(sizeof(vo)) - 48)
            vu += snprintf(vo + vu, sizeof(vo) - vu, "%X/sig%p(%llu) ", b[i].node_rva, b[i].sig,
                           (unsigned long long)b[i].hits[1]);
    }
    log("[indirect] node/signature pairs MAIN issues and VRCAM never: %s", mu ? mo : "(none)");
    log("[indirect] node/signature pairs VRCAM issues and MAIN never: %s", vu ? vo : "(none)");
}

// ---- which PSO draws the holographic sight ---------------------------------------------------
// The reticle is a 6-index instanced quad (PipelineState_29513 in the capture, Forward_NoTXAA,
// depth range 0.9..1.0 -- the first-person weapon layer). To give it real collimated behaviour
// its pixel shader has to be replaced, and a shader can only be swapped where the PSO is CREATED.
// So creation needs a stable name for it. The PSO pointer is not stable across runs; the PS
// bytecode hash is, being a hash of the bytes themselves.
//
// This pass only NAMES it. Nothing is substituted yet and no rendering changes.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_PsoProbe = 1;   // OFF: both hashes are
// known and hardcoded now, and this runs on every 6-index instanced draw -- of which a menu
// issues tens of thousands. Set to 1 to re-identify a shader.
// Identification by REMOVAL. Two pixel shaders draw a 6-index instanced quad once per view per
// frame inside CRenderNode_RenderElements, and their tallies are equally balanced, so counting
// cannot separate them. Setting this to one of the two hashes drops that draw: whichever makes
// the reticle disappear IS the reticle. Instant, reversible, and it also proves the whole
// identification chain end to end before anything is substituted for real.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_SightSkipPs = 0;
// CONFIRMED by removal, 2026-07-29: dropping this pixel shader's draw removes the reticle, and
// dropping the other 6-index quad in the same node (FA07D39A5AFDFBA5) changes nothing visible.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_SightPsHash = 0x66394C5F4B95AB9Aull;
// One-shot: write the original bytecode out, so the replacement can be built against the real
// container -- shader model, input signature and whether this is DXBC or a DXIL blob decide
// whether it is fxc or dxc that has to compile it. Guessing that would waste a build.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_SightPsDump = 1;   // one-shot, long done
struct SightVs { uint64_t hash; std::vector<uint8_t> bytes; bool written = false; };
static std::vector<SightVs> g_sight_vs;
static std::mutex g_sight_vs_mtx;
// Published by the DRAW: the vertex shader the sight actually runs with.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugSightVsUsed = 0;

 void sight_ps_dump(const void* bytes, size_t len, const char* name) {
    char path[MAX_PATH]{};
    if (!GetModuleFileNameA(nullptr, path, MAX_PATH)) return;
    char* slash = strrchr(path, '\\');
    if (!slash) return;
    *(slash + 1) = '\0';
    strcat_s(path, name);
    HANDLE f = CreateFileA(path, GENERIC_WRITE, FILE_SHARE_READ, nullptr,
                           CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (f == INVALID_HANDLE_VALUE) return;
    DWORD wrote = 0;
    WriteFile(f, bytes, static_cast<DWORD>(len), &wrote, nullptr);
    CloseHandle(f);
    // Chunk list straight from the DXBC container header, so the format is known from the log
    // alone: SHEX/SHDR means DXBC (fxc), a DXIL chunk means shader model 6 (dxc).
    char tags[256]; int u = 0; tags[0] = 0;
    __try {
        const uint8_t* b = static_cast<const uint8_t*>(bytes);
        if (len > 32 && memcmp(b, "DXBC", 4) == 0) {
            const uint32_t n = *reinterpret_cast<const uint32_t*>(b + 28);
            const uint32_t* offs = reinterpret_cast<const uint32_t*>(b + 32);
            for (uint32_t i = 0; i < n && i < 24; ++i) {
                if (offs[i] + 4 > len) break;
                if (u < 240) u += snprintf(tags + u, sizeof(tags) - u, "%.4s ", b + offs[i]);
            }
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) { tags[u] = 0; }
    log("[pso] sight shader dumped %zu bytes -> %s   chunks: %s", len, path, tags);
}
struct PsoIds { uint64_t ps, vs; uint32_t ps_len, vs_len; };
static std::unordered_map<void*, PsoIds> g_pso_ids;
static std::mutex g_pso_ids_mtx;
static thread_local ID3D12PipelineState* t_current_pso = nullptr;

 uint64_t fnv1a(const void* p, size_t n) {
    const uint8_t* b = static_cast<const uint8_t*>(p);
    uint64_t h = 1469598103934665603ull;
    for (size_t i = 0; i < n; ++i) { h ^= b[i]; h *= 1099511628211ull; }
    return h;
}

 void pso_ids_record(void* pso, const D3D12_SHADER_BYTECODE& ps,
                           const D3D12_SHADER_BYTECODE& vs) {
    if (!pso) return;
    PsoIds ids{};
    if (ps.pShaderBytecode && ps.BytecodeLength) {
        ids.ps = fnv1a(ps.pShaderBytecode, ps.BytecodeLength);
        ids.ps_len = static_cast<uint32_t>(ps.BytecodeLength);
    }
    if (vs.pShaderBytecode && vs.BytecodeLength) {
        ids.vs = fnv1a(vs.pShaderBytecode, vs.BytecodeLength);
        ids.vs_len = static_cast<uint32_t>(vs.BytecodeLength);
    }
    if (!ids.ps && !ids.vs) return;
    // More than one pipeline uses this pixel shader, and the first one created is NOT the one the
    // sight draws with: the first dump came back with a 3-input vertex shader (POSITION/TEXCOORD/
    // COLOR), which cannot even place a quad in the world. So every vertex shader paired with this
    // pixel shader is cached here, and the DRAW decides afterwards which of them is the real one.
    if (ids.ps == CyberpunkVR_SightPsHash && vs.pShaderBytecode && vs.BytecodeLength) {
        std::lock_guard<std::mutex> lk(g_sight_vs_mtx);
        bool have = false;
        for (auto& e : g_sight_vs) if (e.hash == ids.vs) { have = true; break; }
        if (!have && g_sight_vs.size() < 8) {
            SightVs e;
            e.hash = ids.vs;
            e.bytes.assign(static_cast<const uint8_t*>(vs.pShaderBytecode),
                           static_cast<const uint8_t*>(vs.pShaderBytecode) + vs.BytecodeLength);
            g_sight_vs.push_back(std::move(e));
            log("[pso] sight-PS pipeline #%zu uses VS %016llX (%zu bytes)",
                g_sight_vs.size(), (unsigned long long)ids.vs, vs.BytecodeLength);
        }
    }
    if (CyberpunkVR_SightPsDump && ids.ps == CyberpunkVR_SightPsHash) {
        static std::atomic<bool> s_done{false};
        bool expected = false;
        if (s_done.compare_exchange_strong(expected, true)) {
            sight_ps_dump(ps.pShaderBytecode, ps.BytecodeLength, "cyberpunkvr_sight_ps.bin");
            // (The vertex shader is NOT written here -- see the cache above.)
        }
    }
    std::lock_guard<std::mutex> lk(g_pso_ids_mtx);
    if (g_pso_ids.size() < 65536) g_pso_ids[pso] = ids;
}

// Candidates, tallied. The sight draws exactly ONCE per view per frame, so the row whose hit
// count tracks the frame count is it -- the tally is the check, not a guess from one sighting.
struct SightCand { uint64_t ps; uint32_t node_rva, ps_len; uint64_t hits[2]; };
static std::array<SightCand, 32> g_sight{};
static uint32_t g_sight_n = 0;
static std::mutex g_sight_mtx;

static void sight_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 20000) return;
    SightCand c[32];
    uint32_t n;
    {
        std::lock_guard<std::mutex> lk(g_sight_mtx);
        n = g_sight_n;
        for (uint32_t i = 0; i < n; ++i) c[i] = g_sight[i];
    }
    if (!n) return;
    s_last = now;
    char line[1200];
    int u = 0;
    line[0] = 0;
    for (uint32_t i = 0; i < n; ++i) {
        if (u < static_cast<int>(sizeof(line)) - 70)
            u += snprintf(line + u, sizeof(line) - u, "ps=%016llX(%u)@%X M%llu/V%llu  ",
                          (unsigned long long)c[i].ps, c[i].ps_len, c[i].node_rva,
                          (unsigned long long)c[i].hits[0], (unsigned long long)c[i].hits[1]);
    }
    log("[psoprobe] 6-index instanced quads (%u): %s", n, line);
}

// Returns the PS hash of the draw, 0 when unknown -- the caller uses it to act, not just count.
static uint64_t sight_note(bool vrcam) {
    ID3D12PipelineState* pso = t_current_pso;
    if (!pso) return 0;
    PsoIds ids{};
    {
        std::lock_guard<std::mutex> lk(g_pso_ids_mtx);
        auto it = g_pso_ids.find(pso);
        if (it == g_pso_ids.end()) return 0;
        ids = it->second;
    }
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    const uintptr_t work = t_current_node_work;
    const uint32_t rva = (base && work > base) ? static_cast<uint32_t>(work - base) : 0;
    {
        std::lock_guard<std::mutex> lk(g_sight_mtx);
        uint32_t i = 0;
        for (; i < g_sight_n; ++i) if (g_sight[i].ps == ids.ps && g_sight[i].node_rva == rva) break;
        if (i == g_sight_n) {
            if (g_sight_n >= g_sight.size()) return ids.ps;
            g_sight[g_sight_n++] = { ids.ps, rva, ids.ps_len, {0, 0} };
        }
        ++g_sight[i].hits[vrcam ? 1 : 0];
    }
    if (ids.ps == CyberpunkVR_SightPsHash && ids.vs) {
        CyberpunkVR_DebugSightVsUsed = ids.vs;
        std::lock_guard<std::mutex> lk(g_sight_vs_mtx);
        for (auto& e : g_sight_vs) {
            if (e.hash != ids.vs || e.written) continue;
            e.written = true;
            sight_ps_dump(e.bytes.data(), e.bytes.size(), "cyberpunkvr_sight_vs.bin");
            log("[pso] sight VS IN USE %016llX (%zu bytes) -- that is the one to replace",
                (unsigned long long)e.hash, e.bytes.size());
        }
    }
    sight_report();
    return ids.ps;
}

// The instance buffer sits on a DEFAULT heap -- the first read attempt reported exactly that --
// so it cannot be mapped. But the engine FILLS it with CopyBufferRegion from an upload buffer,
// and that source IS mappable. Remembering the last few (destination range -> source bytes) pairs
// therefore gives a CPU view of it with no new hooks and no readback plumbing.
// The instance buffer is neither mapped nor filled by a copy we can see -- the engine builds it
// on the GPU. So it has to be read back from the GPU, and for that the VA the vertex-buffer view
// carries must be resolved to a resource. There is no API for that, so buffers are recorded as
// they are created. Buffers are exempt from D3D12's state rules (always effectively COMMON, with
// implicit promotion), so the copy below needs no barriers on the engine's resource at all.
struct BufRange { uint64_t va; uint64_t size; ID3D12Resource* res; };
static std::array<BufRange, 512> g_bufs{};
static std::atomic<uint32_t> g_bufs_n{0};
static std::mutex g_bufs_mtx;

 void buf_note(ID3D12Resource* res, uint64_t va, uint64_t size) {
    if (!res || !va || !size) return;
    std::lock_guard<std::mutex> lk(g_bufs_mtx);
    const uint32_t n = g_bufs_n.load(std::memory_order_relaxed);
    if (n >= g_bufs.size()) return;
    g_bufs[n] = { va, size, res };
    g_bufs_n.store(n + 1, std::memory_order_release);
}

static ID3D12Resource* buf_for_va(uint64_t va, uint64_t need, uint64_t* off_out) {
    std::lock_guard<std::mutex> lk(g_bufs_mtx);
    const uint32_t n = g_bufs_n.load(std::memory_order_acquire);
    for (uint32_t i = 0; i < n; ++i) {
        const BufRange& b = g_bufs[i];
        if (va >= b.va && va + need <= b.va + b.size) { *off_out = va - b.va; return b.res; }
    }
    return nullptr;
}

struct FilledRange { uint64_t va; uint64_t bytes; const uint8_t* cpu; };
static std::array<FilledRange, 32> g_filled{};
static std::atomic<uint32_t> g_filled_next{0};
static std::mutex g_filled_mtx;

static void filled_note(uint64_t va, uint64_t bytes, const uint8_t* cpu) {
    if (!va || !cpu || !bytes) return;
    std::lock_guard<std::mutex> lk(g_filled_mtx);
    const uint32_t i = g_filled_next.fetch_add(1, std::memory_order_relaxed) % g_filled.size();
    g_filled[i] = { va, bytes, cpu };
}

// SEH cannot share a frame with objects that unwind, and filled_note takes a lock -- hence the
// split. Same reason the stream walker and the grading commit are their own functions.
static void filled_note_guarded(ID3D12Resource* dst, uint64_t dst_off, uint64_t bytes,
                                const uint8_t* cpu) {
    uint64_t va = 0;
    __try { va = dst->GetGPUVirtualAddress(); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    if (va) filled_note(va + dst_off, bytes, cpu);
}

 const uint8_t* filled_cpu_for_va(uint64_t va, uint64_t need) {
    std::lock_guard<std::mutex> lk(g_filled_mtx);
    for (const FilledRange& f : g_filled)
        if (f.cpu && va >= f.va && va + need <= f.va + f.bytes) return f.cpu + (va - f.va);
    return nullptr;
}

// The instance stream, per recording thread.
static thread_local uint64_t t_inst_va = 0;
static thread_local uint32_t t_inst_stride = 0;
// OFF: a GetGPUVirtualAddress() call and a mutex on every CopyBufferRegion, every descriptor
// table bind and every resource creation -- thousands a frame. It answered its question; the
// cost it leaves behind is uneven frame time.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_SightAxisProbe = 1;

 void STDMETHODCALLTYPE hk_IASetVertexBuffers(ID3D12GraphicsCommandList* self,
        UINT start, UINT num, const D3D12_VERTEX_BUFFER_VIEW* views) {
    const CommandListVtableHook* e = command_list_hook_entry(self);
    if (!e || !e->iavb_original) return;
    if (CyberpunkVR_SightAxisProbe && views && num >= 1 && start == 7) {
        t_inst_va = views[0].BufferLocation;
        t_inst_stride = views[0].StrideInBytes;
    }
    e->iavb_original(self, start, num, views);
}

// Decode the sight's world orientation for this view and print it once. The three rows are the
// instance rotation; the optical axis is the one the vertex shader picks -- the thin bounding-box
// axis -- so printing all three lets the two views be compared directly.
static ID3D12Resource* g_axis_rb[2] = {nullptr, nullptr};
static void* g_axis_map[2] = {nullptr, nullptr};
static std::atomic<int> g_axis_state[2] = {};       // 0 idle, 1 copy recorded, 2 reported
static uint64_t g_axis_when[2] = {0, 0};
static UINT g_axis_inst[2] = {0, 0};

// Print whatever landed in the readback, once the copy has surely retired.
static void sight_axis_drain() {
    for (int v = 0; v < 2; ++v) {
        if (g_axis_state[v].load(std::memory_order_acquire) != 1) continue;
        if (GetTickCount64() - g_axis_when[v] < 300) continue;   // a few frames is plenty
        g_axis_state[v].store(2, std::memory_order_release);
        const float* r = static_cast<const float*>(g_axis_map[v]);
        if (!r) continue;
        // THE PART THAT WAS NEVER READ. Rows 0..2 gave the rotation, which came out identical
        // in both views; the .w of each row is the instance's TRANSLATION, int32 fixed point at
        // 1/131072, rebased per view. And the rebase cancels: the rebase origin IS the view's
        // camera position (measured: 417378622/131072 = 3184.34617 = _25_m0[37].x), and
        // wp = _25_m0[37] + (instW - rebase)/131072, so wp = instW/131072 outright. One divide
        // and we have the sight's WORLD position for each view, with no constant buffer needed.
        //
        // This is the fork. Identical between views => the weapon is placed once in the world,
        // the sight axis passes through ONE eye, and the other is an IPD off it. Differing by
        // the eye separation => the weapon is placed per view, both eyes sit on their own axis,
        // and the zero distance can have no effect -- which is what was observed.
        const uint32_t* u = static_cast<const uint32_t*>(g_axis_map[v]);
        const double kFp = 1.0 / 131072.0;
        log("[sightaxis] %-5s inst=%u  row0=(%+.6f %+.6f %+.6f)  row1=(%+.6f %+.6f %+.6f)  "
            "row2=(%+.6f %+.6f %+.6f)  world=(%.5f %.5f %.5f)",
            v ? "VRCAM" : "MAIN", g_axis_inst[v],
            r[0], r[1], r[2], r[4], r[5], r[6], r[8], r[9], r[10],
            static_cast<int32_t>(u[3]) * kFp,
            static_cast<int32_t>(u[7]) * kFp,
            static_cast<int32_t>(u[11]) * kFp);
    }
}

// Re-arm both slots at once, every few seconds. Sampling the two views in DIFFERENT frames
// would let the weapon's own sway (centimetres) drown the 65 mm we are looking for; re-arming
// them together keeps each pair one frame apart at worst, and repeating it makes a one-off
// coincidence visible as noise instead of being mistaken for the answer.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_SightAxisRepeatMs = 4000;

static void sight_axis_readback(ID3D12GraphicsCommandList* list, bool vrcam, UINT sinst,
                                uint64_t va) {
    const int v = vrcam ? 1 : 0;
    if (CyberpunkVR_SightAxisRepeatMs > 0 && !vrcam) {
        static uint64_t s_armed = 0;
        const uint64_t now = GetTickCount64();
        if (g_axis_state[0].load(std::memory_order_acquire) == 2 &&
            g_axis_state[1].load(std::memory_order_acquire) == 2 &&
            now - s_armed > static_cast<uint64_t>(CyberpunkVR_SightAxisRepeatMs)) {
            s_armed = now;
            g_axis_state[0].store(0, std::memory_order_release);
            g_axis_state[1].store(0, std::memory_order_release);
        }
    }
    if (g_axis_state[v].load(std::memory_order_acquire) != 0) return;
    const CommandListVtableHook* e = command_list_hook_entry(list);
    if (!e || !e->cbr_original || !g_game_device) return;
    uint64_t off = 0;
    ID3D12Resource* res = buf_for_va(va, 64, &off);
    if (!res) {
        static uint64_t s_last = 0;
        const uint64_t now = GetTickCount64();
        if (!s_last || now - s_last > 10000) {
            s_last = now;
            log("[sightaxis] va=%llX not in any recorded buffer (created before the hook?)",
                (unsigned long long)va);
        }
        return;
    }
    if (!g_axis_rb[v]) {
        D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_READBACK;
        hp.CreationNodeMask = hp.VisibleNodeMask = 1;
        D3D12_RESOURCE_DESC bd{};
        bd.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
        bd.Width = 64; bd.Height = 1; bd.DepthOrArraySize = 1; bd.MipLevels = 1;
        bd.Format = DXGI_FORMAT_UNKNOWN; bd.SampleDesc.Count = 1;
        bd.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        if (FAILED(g_game_device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &bd,
                D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&g_axis_rb[v]))) ||
            !g_axis_rb[v]) return;
        if (FAILED(g_axis_rb[v]->Map(0, nullptr, &g_axis_map[v]))) { g_axis_map[v] = nullptr; return; }
    }
    e->cbr_original(list, g_axis_rb[v], 0, res, off, 64);
    g_axis_inst[v] = sinst;
    g_axis_when[v] = GetTickCount64();
    g_axis_state[v].store(1, std::memory_order_release);
}

static void sight_axis_note(bool vrcam, UINT sinst) {
    static bool s_done[2] = {false, false};
    const int v = vrcam ? 1 : 0;
    if (s_done[v] || !t_inst_va || t_inst_stride < 48) return;
    const uint64_t va = t_inst_va + static_cast<uint64_t>(sinst) * t_inst_stride;
    const uint8_t* p = upload_cpu_for_va(va, t_inst_stride);
    if (!p) p = filled_cpu_for_va(va, t_inst_stride);
    if (!p) {
        static uint64_t s_last = 0;
        const uint64_t now = GetTickCount64();
        (void)s_last; (void)now;
        return;                       // the caller falls back to the GPU readback
    }
    float r[12];
    __try { memcpy(r, p, sizeof(r)); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    s_done[v] = true;
    log("[sightaxis] %-5s inst=%u  row0=(%+.6f %+.6f %+.6f)  row1=(%+.6f %+.6f %+.6f)  "
        "row2=(%+.6f %+.6f %+.6f)",
        vrcam ? "VRCAM" : "MAIN", sinst,
        r[0], r[1], r[2], r[4], r[5], r[6], r[8], r[9], r[10]);
}

 void STDMETHODCALLTYPE hk_SetPipelineState(ID3D12GraphicsCommandList* self,
        ID3D12PipelineState* pso) {
    const CommandListVtableHook* e = command_list_hook_entry(self);
    if (!e || !e->setpso_original) return;
    t_current_pso = pso;
    e->setpso_original(self, pso);
}

// ---- draw census: which nodes draw for one view and never for the other ---------------------
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DrawCensus = 1;   // OFF: [draw] per-node draw census + imbalance
struct DrawBin { uint32_t node_rva; uint64_t hits[2]; };
static std::array<DrawBin, 96> g_draw{};
static uint32_t g_draw_n = 0;
static std::mutex g_draw_mtx;

static void draw_census_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 15000) return;
    DrawBin b[96];
    uint32_t n;
    {
        std::lock_guard<std::mutex> lk(g_draw_mtx);
        n = g_draw_n;
        for (uint32_t i = 0; i < n; ++i) b[i] = g_draw[i];
    }
    bool anym = false, anyv = false;
    for (uint32_t i = 0; i < n; ++i) {
        if (b[i].hits[0]) anym = true;
        if (b[i].hits[1]) anyv = true;
    }
    if (!anym || !anyv) return;
    s_last = now;
    for (int pass = 0; pass < 2; ++pass) {
        char line[1100];
        int u = 0, c = 0;
        line[0] = 0;
        for (uint32_t i = 0; i < n; ++i) {
            const uint64_t mine = b[i].hits[pass], other = b[i].hits[pass ^ 1];
            if (!mine || other) continue;
            ++c;
            if (u < static_cast<int>(sizeof(line)) - 28)
                u += snprintf(line + u, sizeof(line) - u, "%X(%llu) ",
                              b[i].node_rva, (unsigned long long)mine);
        }
        log("[draw] nodes that DRAW for %s and never for %s (%d of %u): %s",
            pass ? "VRCAM" : "MAIN", pass ? "MAIN" : "VRCAM", c, n, c ? line : "(none)");
    }
    // Exclusive nodes are only half the story: a node can draw for both views and still do
    // almost nothing for one of them. That is the shape the old audit hinted at for
    // RenderVisionElements (11012 vs 1607), and printing only the exclusive set hides it --
    // the same blind spot that cost this project two wrong turns today in other tools.
    char line[1100];
    int u = 0, c = 0;
    line[0] = 0;
    for (uint32_t i = 0; i < n; ++i) {
        const uint64_t m = b[i].hits[0], v = b[i].hits[1];
        if (!m || !v) continue;
        const uint64_t hi = m > v ? m : v, lo = m > v ? v : m;
        if (hi < lo * 3) continue;
        ++c;
        if (u < static_cast<int>(sizeof(line)) - 44)
            u += snprintf(line + u, sizeof(line) - u, "%X(M%llu/V%llu %.1fx) ", b[i].node_rva,
                          (unsigned long long)m, (unsigned long long)v,
                          static_cast<double>(hi) / static_cast<double>(lo));
    }
    log("[draw] nodes both views draw but >=3x imbalanced (%d): %s", c, c ? line : "(none)");
}

static void draw_census_note(bool vrcam) {
    if (!g_exe_base) return;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    const uintptr_t work = t_current_node_work;
    if (work <= base) return;
    const uint32_t rva = static_cast<uint32_t>(work - base);
    {
        std::lock_guard<std::mutex> lk(g_draw_mtx);
        uint32_t i = 0;
        for (; i < g_draw_n; ++i) if (g_draw[i].node_rva == rva) break;
        if (i == g_draw_n) {
            if (g_draw_n >= g_draw.size()) return;
            g_draw[g_draw_n++] = { rva, {0, 0} };
        }
        ++g_draw[i].hits[vrcam ? 1 : 0];
    }
    draw_census_report();
}

// ---- WHAT EACH VIEW PUTS INTO ITS OWN SHADOW CASCADE -------------------------------------------
//
// Shadows cast by fences, plants and trees differ between the eyes: a piece of shadow is present in
// one and missing in the other, in different places. A capture settled the first half of that -- the
// cascade draws come in TWO separated runs per frame (RenderCascade0: events 19657..20440, then
// 46391..47167, with the other view's scene passes in between), so each view rasterises its OWN
// cascade rather than sharing one. Cutting the cascade node for the second view confirmed it from the
// other side: that eye lost sun shadows entirely, so the render target is per graph.
//
// The same capture showed the two runs drawing ALMOST the same thing, differing in instance counts --
// 373 against 376 of one 33-index mesh, 554 against 556, 70 against 71 -- which is a handful of
// instanced foliage cards in one eye's shadow map and not the other's.
//
// BUT THAT CAPTURE CANNOT BE TRUSTED FOR THIS, and saying so is the point of this probe: it was taken
// with the two eyes at different resolutions (3072 against 2560) because Nsight would not record
// otherwise, and LOD selection and culling margins both scale with resolution. The difference may
// therefore be an artefact of the capture conditions rather than the reported defect.
//
// So this counts the same thing live, where the eyes are configured alike: per view, how many draws
// the cascade node issues, how many instances in total, and an order-independent sum over the draw
// arguments -- so a difference in WHICH draws are issued shows up even when the totals match. Counters
// are per interval, reset after each report, because two numbers from the same seconds compare and two
// running totals from different seconds do not.
// DEFAULT 0: it counted every cascade draw in the frame and its job is done (the two views draw the
// same casters, 94002 against 94004). Kept for repeating that measurement.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_CascadeCensus = 0;
namespace {
std::atomic<uint64_t> g_casc_draws[2];
std::atomic<uint64_t> g_casc_inst[2];
std::atomic<uint64_t> g_casc_sum[2];

void cascade_note(UINT idx, UINT inst, UINT sinst, bool vrcam) {
    const int v = vrcam ? 1 : 0;
    g_casc_draws[v].fetch_add(1, std::memory_order_relaxed);
    g_casc_inst[v].fetch_add(inst, std::memory_order_relaxed);
    // Commutative on purpose: the two views submit in their own order, and order is not the question.
    g_casc_sum[v].fetch_add(static_cast<uint64_t>(idx) * 31u + inst * 7u + sinst,
                            std::memory_order_relaxed);
}

void cascade_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    const bool first = (s_last == 0);
    s_last = now;
    const uint64_t dm = g_casc_draws[0].exchange(0, std::memory_order_relaxed);
    const uint64_t dv = g_casc_draws[1].exchange(0, std::memory_order_relaxed);
    const uint64_t im = g_casc_inst[0].exchange(0, std::memory_order_relaxed);
    const uint64_t iv = g_casc_inst[1].exchange(0, std::memory_order_relaxed);
    const uint64_t hm = g_casc_sum[0].exchange(0, std::memory_order_relaxed);
    const uint64_t hv = g_casc_sum[1].exchange(0, std::memory_order_relaxed);
    if (first) return;                      // the first interval is a partial one; it proves nothing
    log("[cascade] per 5 s: MAIN draws=%llu inst=%llu | VRCAM draws=%llu inst=%llu | "
        "per-draw inst avg M=%.2f V=%.2f | argsum M=%llu V=%llu",
        (unsigned long long)dm, (unsigned long long)im,
        (unsigned long long)dv, (unsigned long long)iv,
        dm ? (double)im / (double)dm : 0.0, dv ? (double)iv / (double)dv : 0.0,
        (unsigned long long)hm, (unsigned long long)hv);
}
}  // namespace

// The duplicate cascade rasterisation, withheld from MAIN. Both eyes bind the same atlas descriptor from an
// identical record (see the depth-target probe in Capture.cpp), so MAIN's copy of it is redundant work --
// but the earlier attempt to save it by cutting the NODE made shadows twitch, because the node's binds and
// transitions went with it. Here the node still runs and only the draws are dropped, so the graph keeps
// every barrier it declared. Paired with skipping MAIN's ClearShadowCascades, which is what leaves the
// second view's contents in the atlas for MAIN to sample. See CyberpunkVR_CascadeSaveMain in ViewReuse.cpp.
extern "C" __declspec(dllexport) extern int32_t  CyberpunkVR_CascadeSaveMain;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugCascadeDrawsSaved;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCascadeClearsSaved = 0;
// Draws seen at the cascade node, PER SIDE, counted before anything is withheld.
//
// Built because four captures and a counter disagreed. Every capture -- including one taken before this knob
// existed -- shows cascade draws only in the FIRST view's half of the frame and none in MAIN's, while
// DebugCascadeDrawsSaved says 286 of MAIN's are withheld every frame. Both cannot be true, and the difference
// between them is entirely about which side the port thinks it is on when the cascade node runs. So the port
// counts what it actually sees instead of being argued about: three numbers, and each of the competing stories
// predicts a different one.
//
//   side1 large, side0 zero   MAIN never draws cascades here at all, and the withheld count is mislabelled:
//                             the side test is reading the wrong thing at this node
//   both large                both views draw them, the captures are missing MAIN's, and a capture cannot be
//                             used to judge command-list work (RenderDoc records above our hook)
//   side0 large, side1 zero   the sides are inverted and the port has been withholding the FIRST view's work
// EVERY counter and report below is behind this, and the gate zeroes it when DEBUG is unticked. The
// reason is measurable rather than tidiness: the per-side census called GetTickCount64 on EVERY draw at
// the cascade node -- hundreds a frame -- which is exactly the kind of cost this file warns about at the
// top. With the box unticked cascade_draw_withheld() now does one node compare and the withheld
// decision, and nothing else. The FIXES are not gated: they have to work in an ordinary session.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CascSideCensus = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCascadeDrawsSide0 = 0;   // MAIN
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCascadeDrawsSide1 = 0;   // second view
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCascadeDrawsSideX = 0;   // neither eye

extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugCaptureMarkers;
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_CascSideCensus;

static void cascade_draw_seen_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    log("[cascside] draws AT the cascade node by side: MAIN=%llu secondView=%llu neither=%llu "
        "| withheld=%llu clears=%llu | markersEmitted=%llu | vrcamFlag=%d viewSide=%d",
        (unsigned long long)CyberpunkVR_DebugCascadeDrawsSide0,
        (unsigned long long)CyberpunkVR_DebugCascadeDrawsSide1,
        (unsigned long long)CyberpunkVR_DebugCascadeDrawsSideX,
        (unsigned long long)CyberpunkVR_DebugCascadeDrawsSaved,
        (unsigned long long)CyberpunkVR_DebugCascadeClearsSaved,
        (unsigned long long)CyberpunkVR_DebugCaptureMarkers,
        t_vrcam_node_active ? 1 : 0, (int)t_view_side);
}

static bool cascade_draw_withheld() {
    if (!g_exe_base) return false;
    if (t_current_node_work != reinterpret_cast<uintptr_t>(g_exe_base) + CASCADE_NODE_RVA) return false;
    // Counted for every side, and BEFORE the knob is consulted, so the numbers describe the engine rather
    // than the effect of our own switch. Diagnostic only: silent with DEBUG unticked.
    if (CyberpunkVR_CascSideCensus) {
        volatile LONG64* slot =
            (t_view_side == 0) ? reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCascadeDrawsSide0)
          : (t_view_side == 1) ? reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCascadeDrawsSide1)
                               : reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCascadeDrawsSideX);
        InterlockedIncrement64(slot);
        cascade_draw_seen_report();
    }
    if (!CyberpunkVR_CascadeSaveMain || t_vrcam_node_active) return false;
    if (CyberpunkVR_CascSideCensus)
        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCascadeDrawsSaved));
    return true;
}

// MAIN's clear of the shadow atlas, withheld -- the other half of the same saving, and the half the previous
// attempt got wrong by cutting the node that issues it. The node still runs, so the atlas still transitions
// NON_PIXEL_SHADER_RESOURCE -> DEPTH_WRITE and back exactly as the graph declared; only the clear command
// itself is dropped, so the depth the second view rasterised this frame survives for MAIN to sample.
//
// ---- VIEW AND NODE MARKERS, so a capture labels itself -------------------------------------------
//
// Built because a question could not be answered from three captures in a row: with MAIN's cascade draws
// withheld, a block of cascade draws still appeared in the middle of the frame, and NOTHING in the file said
// whether that region belonged to MAIN's graph or the tail of the second view's. The two readings could not be
// separated, because a D3D12 capture is a flat list of five thousand commands with no idea that this port runs
// the graph twice.
//
// So the port says it. One PIX marker whenever the (command list, node, view, cascade) tuple changes.
//
// AND IT DOES NOT REACH A RENDERDOC CAPTURE AS BUILT -- measured: markers found 0, with DEBUG armed and the
// counter proving they were emitted. Our vtable hook sits BELOW RenderDoc's wrapper, so the marker goes into
// the real command list and the capture layer above never sees it. The same ordering is why a draw this file
// withholds still appears in a capture: RenderDoc serialises what the GAME called, then our hook drops it.
//
// To fix it the marker has to be emitted on the object the GAME holds, i.e. by hooking CreateCommandList on the
// device vtable the game uses (which DEPTH-DIAG already reaches) and then that list's vtable -- in a capture run
// that is RenderDoc's wrapper. Left as is for now, because the thing markers were built to answer got answered
// another way: read whose camera is bound (CameraShaderConsts float 144, the eyes 0.0640 m apart in X) and every
// region labels itself, on every capture already taken. Kept because they DO show in a PIX or Nsight capture,
// which hook at the driver level rather than above us.
//
// Emitted BEFORE the withheld-draw test on purpose. A node whose draws are all dropped otherwise records
// nothing at all and vanishes from the capture; with the marker it still appears, empty and labelled, which is
// exactly the evidence the cascade-ownership question needs.
//
// SetMarker rather than BeginEvent/EndEvent: a marker needs no matching close, so there is no path by which
// this can leave a command list with an unbalanced scope. Metadata 1 is the PIX convention for a
// null-terminated ANSI string.
// DEFAULT 0. This is RenderDoc scaffolding and it shipped armed. Every change of
// (command list, node, view, cascade) formats a label with sprintf_s and calls SetMarker on one of
// the game's own command lists -- and a camera feed or a monitor is an EXTRA view (it classifies as
// neither-eye), so each active one multiplies the marker count. Turn it on for a capture; it has no
// business costing frame time in play.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CaptureMarkers = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCaptureMarkers = 0;
extern "C" __declspec(dllexport) const char* CyberpunkVR_ProfNodeName(uint32_t rva);

namespace {
constexpr UINT kPixAnsiMarker = 1;

// One dedupe state per hook path. Sharing one would make the key alternate between the real list and
// the wrapper on every call, and then EVERY draw would emit a marker instead of every boundary.
struct MarkerState {
    void*    list = nullptr;
    uint32_t node = 0xFFFFFFFFu;
    int32_t  side = -2;
    int32_t  casc = -2;
};
thread_local MarkerState t_mark_real;
thread_local MarkerState t_mark_wrap;

void** g_wrapper_vtable = nullptr;   // set once the game-facing (wrapped) list vtable is hooked

void maybe_marker_in(ID3D12GraphicsCommandList* self, MarkerState& mk) {
    if (!CyberpunkVR_CaptureMarkers || !self || !g_exe_base) return;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    if (t_current_node_work <= base) return;              // outside a node: nothing to name it after
    const uint32_t rva = static_cast<uint32_t>(t_current_node_work - base);
    if (self == mk.list && rva == mk.node && t_view_side == mk.side && t_cascade_idx == mk.casc)
        return;
    mk.list = self;
    mk.node = rva;
    mk.side = t_view_side;
    mk.casc = t_cascade_idx;

    const char* side = (t_view_side == 0) ? "MAIN"
                     : (t_view_side == 1) ? "VRCAM"
                                          : "neither-eye";
    const char* node = CyberpunkVR_ProfNodeName(rva);
    char label[160];
    if (t_cascade_idx >= 0)
        sprintf_s(label, "%s | %s [cascade %d]", side, node ? node : "?", (int)t_cascade_idx);
    else
        sprintf_s(label, "%s | %s", side, node ? node : "?");
    // WHOSE object are we writing into? Said once, because it is the whole question. RenderDoc serialises
    // SetMarker unconditionally in capture mode and PIX_EVENT_ANSI_VERSION is 1, both read out of its own
    // source -- so a marker that never appears in a capture can only mean this `self` is the REAL D3D12 list
    // and not the wrapper the game records into. The module that owns the vtable's first entry settles it:
    // renderdoc.dll means we are above the capture layer, d3d12core/nvwgf2umx means below.
    static std::atomic<int> said{0};
    if (said.exchange(1) == 0) {
        const void* vt = *reinterpret_cast<const void* const*>(self);
        const void* first = vt ? *reinterpret_cast<const void* const*>(vt) : nullptr;
        char modname[MAX_PATH] = "(unknown)";
        HMODULE mod = nullptr;
        if (first && GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                                        GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                                        reinterpret_cast<LPCSTR>(first), &mod) && mod) {
            char full[MAX_PATH] = {};
            if (GetModuleFileNameA(mod, full, MAX_PATH)) {
                const char* slash = strrchr(full, '\\');
                strncpy_s(modname, slash ? slash + 1 : full, _TRUNCATE);
            }
        }
        log("[cascmark] first marker: list=%p vtable=%p vtable[0] belongs to %s | label='%s'",
            (void*)self, (void*)vt, modname, label);
    }
    __try {
        self->SetMarker(kPixAnsiMarker, label, static_cast<UINT>(strlen(label) + 1));
    } __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCaptureMarkers));
}
}  // namespace

// The REAL-list path. Its markers are invisible to RenderDoc -- measured: vtable[0] belongs to
// D3D12Core.dll -- so once the wrapper vtable is hooked this stops emitting rather than burning work for
// nothing. It still earns its keep under PIX and Nsight, which intercept below us.
void maybe_marker(ID3D12GraphicsCommandList* self) {
    if (g_wrapper_vtable) return;
    maybe_marker_in(self, t_mark_real);
}

// ---- THE SAME MARKERS, ON THE OBJECT THE GAME ACTUALLY RECORDS INTO ------------------------------
//
// Measured in this order: markers emitted 198154 times, markers in the capture 0, then
// [cascmark] first marker: vtable[0] belongs to D3D12Core.dll. RenderDoc serialises SetMarker whenever it is
// in capture mode and PIX_EVENT_ANSI_VERSION is 1 -- both read out of its own source -- so the only remaining
// explanation was the one that proved true: our vtable hook sits on the REAL list, underneath the wrapper the
// game records into.
//
// The wrapper VTABLE is reachable without ever finding an engine pointer. The device obtained in
// CreateSwapChainForHwnd comes from the queue the GAME passed, so it is the game-facing device -- wrapped when
// a capture layer is present. One throwaway command list created from it therefore carries the wrapper vtable,
// and every list the engine already built shares it. Four patched slots and the markers land above the layer.
//
// Installed ONLY when that vtable belongs to renderdoc.dll. In an ordinary session there is no wrapper, the
// vtable is D3D12Core.dll which this file already hooks elsewhere, and patching it twice would double every
// count. Normal play is therefore untouched by construction, which is also correct: markers exist for captures.
namespace {
struct WrapperHooks {
    PFN_DrawInstanced         draw = nullptr;
    PFN_DrawIndexedInstanced  drawidx = nullptr;
    PFN_Dispatch              disp = nullptr;
    PFN_ClearDepthStencilView cleardsv = nullptr;
};
WrapperHooks g_wrap;

void STDMETHODCALLTYPE wrap_DrawInstanced(ID3D12GraphicsCommandList* self, UINT v, UINT i, UINT sv, UINT si) {
    maybe_marker_in(self, t_mark_wrap);
    g_wrap.draw(self, v, i, sv, si);
}
void STDMETHODCALLTYPE wrap_DrawIndexedInstanced(ID3D12GraphicsCommandList* self, UINT n, UINT i, UINT si,
                                                 INT bv, UINT sinst) {
    maybe_marker_in(self, t_mark_wrap);
    g_wrap.drawidx(self, n, i, si, bv, sinst);
}
void STDMETHODCALLTYPE wrap_Dispatch(ID3D12GraphicsCommandList* self, UINT x, UINT y, UINT z) {
    maybe_marker_in(self, t_mark_wrap);
    g_wrap.disp(self, x, y, z);
}
void STDMETHODCALLTYPE wrap_ClearDepthStencilView(ID3D12GraphicsCommandList* self,
        D3D12_CPU_DESCRIPTOR_HANDLE dsv, D3D12_CLEAR_FLAGS f, FLOAT d, UINT8 st8, UINT nr,
        const D3D12_RECT* r) {
    maybe_marker_in(self, t_mark_wrap);
    g_wrap.cleardsv(self, dsv, f, d, st8, nr, r);
}

bool patch_vtable_slot(void** vt, UINT slot, void* hook, void** saved) {
    DWORD old = 0;
    if (!VirtualProtect(&vt[slot], sizeof(void*), PAGE_READWRITE, &old)) return false;
    *saved = vt[slot];
    vt[slot] = hook;
    DWORD junk = 0;
    VirtualProtect(&vt[slot], sizeof(void*), old, &junk);
    return true;
}

// Base name of the module owning a code address. 92 is the path separator, written as its code point so this
// file stays free of escape-sequence surprises.
const char* module_base_name(const void* addr, char* buf, size_t n) {
    HMODULE mod = nullptr;
    if (!addr || !GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                                     GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                                     reinterpret_cast<LPCSTR>(addr), &mod) || !mod)
        return "(unknown)";
    char full[MAX_PATH] = {};
    if (!GetModuleFileNameA(mod, full, MAX_PATH)) return "(unknown)";
    const char* slash = strrchr(full, 92);
    strncpy_s(buf, n, slash ? slash + 1 : full, _TRUNCATE);
    return buf;
}
}  // namespace

extern "C" void RegisterGameFacingListVtable(ID3D12Device* device) {
    if (!device || g_wrapper_vtable) return;
    ID3D12CommandAllocator* alloc = nullptr;
    if (FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&alloc))) || !alloc)
        return;
    ID3D12GraphicsCommandList* list = nullptr;
    const HRESULT hr = device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, alloc, nullptr,
                                                 IID_PPV_ARGS(&list));
    if (SUCCEEDED(hr) && list) {
        void** vt = *reinterpret_cast<void***>(list);
        char modbuf[MAX_PATH] = {};
        const char* mod = module_base_name(vt ? vt[0] : nullptr, modbuf, sizeof(modbuf));
        if (vt && _stricmp(mod, "renderdoc.dll") == 0) {
            const bool ok =
                patch_vtable_slot(vt, 12, reinterpret_cast<void*>(&wrap_DrawInstanced),
                                  reinterpret_cast<void**>(&g_wrap.draw)) &&
                patch_vtable_slot(vt, 13, reinterpret_cast<void*>(&wrap_DrawIndexedInstanced),
                                  reinterpret_cast<void**>(&g_wrap.drawidx)) &&
                patch_vtable_slot(vt, 14, reinterpret_cast<void*>(&wrap_Dispatch),
                                  reinterpret_cast<void**>(&g_wrap.disp)) &&
                patch_vtable_slot(vt, 47, reinterpret_cast<void*>(&wrap_ClearDepthStencilView),
                                  reinterpret_cast<void**>(&g_wrap.cleardsv));
            if (ok) g_wrapper_vtable = vt;
            log("[cascmark] game-facing list vtable=%p owned by %s -- marker hooks %s",
                (void*)vt, mod, ok ? "INSTALLED, markers now land in the capture" : "FAILED to patch");
        } else {
            log("[cascmark] game-facing list vtable=%p owned by %s -- no capture layer above us, wrapper "
                "hooks not needed", (void*)vt, mod);
        }
        list->Release();
    }
    alloc->Release();
}

// Whether this knob DID anything, in one line. "The picture is fine" and "the knob never fired" look identical
// from the outside, and this port has already spent rounds on probes that never reached their target -- so the
// counters get said out loud. NOT gated behind a diagnostic flag: it confirms a fix rather than measuring one,
// it is one line per five seconds, and with the launcher's DEBUG box unticked it is the only thing that can
// distinguish a saving from a no-op.
static void cascade_save_report() {
    if (!CyberpunkVR_CascadeSaveMain || !CyberpunkVR_CascSideCensus) return;
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    log("[cascsave] MAIN's cascade work withheld so far: draws=%llu clears=%llu",
        (unsigned long long)CyberpunkVR_DebugCascadeDrawsSaved,
        (unsigned long long)CyberpunkVR_DebugCascadeClearsSaved);
}

// Gated on the CLEAR node, not the render node: a depth clear anywhere else in the frame is somebody else's.
void STDMETHODCALLTYPE hk_ClearDepthStencilView(ID3D12GraphicsCommandList* self,
        D3D12_CPU_DESCRIPTOR_HANDLE dsv, D3D12_CLEAR_FLAGS flags, FLOAT depth, UINT8 stencil,
        UINT rects, const D3D12_RECT* rect) {
    const CommandListVtableHook* e = command_list_hook_entry(self);
    if (!e || !e->cleardsv_original) return;
    maybe_marker(self);
    cascade_save_report();
    if (CyberpunkVR_CascadeSaveMain && g_exe_base && !t_vrcam_node_active &&
            t_current_node_work == reinterpret_cast<uintptr_t>(g_exe_base) + CASCADE_CLEAR_NODE_RVA) {
        if (CyberpunkVR_CascSideCensus)
            InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCascadeClearsSaved));
        return;
    }
    e->cleardsv_original(self, dsv, flags, depth, stencil, rects, rect);
}

 void STDMETHODCALLTYPE hk_DrawInstanced(ID3D12GraphicsCommandList* self,
        UINT vtx, UINT inst, UINT sv, UINT si) {
    const CommandListVtableHook* e = command_list_hook_entry(self);
    if (!e || !e->draw_original) return;
    maybe_marker(self);
    if (cascade_draw_withheld()) return;
    e->draw_original(self, vtx, inst, sv, si);
    if (CyberpunkVR_DrawCensus) draw_census_note(t_vrcam_node_active);
}

 void STDMETHODCALLTYPE hk_DrawIndexedInstanced(ID3D12GraphicsCommandList* self,
        UINT idx, UINT inst, UINT si, INT bv, UINT sinst) {
    const CommandListVtableHook* e = command_list_hook_entry(self);
    if (!e || !e->drawidx_original) return;
    maybe_marker(self);
    if (cascade_draw_withheld()) return;
    // The sight's exact draw shape, from the capture: 6 indices, one instance, no index or vertex
    // offset, and an instance slot picked by StartInstanceLocation. Resolved BEFORE the call,
    // because the skip test has to be able to withhold it.
    if (CyberpunkVR_SightAxisProbe && idx == 6 && inst == 1 && si == 0 && bv == 0 && sinst != 0) {
        // Only for the sight's own pixel shader, so the many other 6-index quads cost nothing.
        ID3D12PipelineState* pso = t_current_pso;
        if (pso) {
            uint64_t ps = 0;
            {
                std::lock_guard<std::mutex> lk(g_pso_ids_mtx);
                auto it = g_pso_ids.find(pso);
                if (it != g_pso_ids.end()) ps = it->second.ps;
            }
            if (ps == CyberpunkVR_SightPsHash) {
                sight_axis_note(t_vrcam_node_active, sinst);
                sight_axis_readback(self, t_vrcam_node_active, sinst,
                                    t_inst_va + static_cast<uint64_t>(sinst) * t_inst_stride);
                sight_axis_drain();
            }
        }
    }
    if (CyberpunkVR_PsoProbe && idx == 6 && inst == 1 && si == 0 && bv == 0 && sinst != 0) {
        const uint64_t ps = sight_note(t_vrcam_node_active);
        if (ps && ps == CyberpunkVR_SightSkipPs) return;
    }
    e->drawidx_original(self, idx, inst, si, bv, sinst);
    if (CyberpunkVR_DrawCensus) draw_census_note(t_vrcam_node_active);
    // The shadow cascade, counted per view. One compare on every other draw in the frame.
    if (CyberpunkVR_CascadeCensus && g_exe_base &&
            t_current_node_work == reinterpret_cast<uintptr_t>(g_exe_base) + CASCADE_NODE_RVA) {
        cascade_note(idx, inst, sinst, t_vrcam_node_active);
        cascade_report();
    }
}

// [EI-DIAG] How often this hook drops the game's draw, and how often it declines to issue one that
// would kill the process. Exported so they can be read without waiting for a log line.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugEiNoEntry = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugEiNullArgs = 0;

 void STDMETHODCALLTYPE hk_ExecuteIndirect(ID3D12GraphicsCommandList* self,
        ID3D12CommandSignature* sig, UINT maxCount, ID3D12Resource* args, UINT64 argOff,
        ID3D12Resource* cnt, UINT64 cntOff) {
    const CommandListVtableHook* e = command_list_hook_entry(self);
    if (!e || !e->indirect_original) {
        // Returning here does not just skip our census -- IT NEVER ISSUES THE GAME'S DRAW. That is a
        // defect independent of anything else in this function, and it had never been counted
        // (satyaloka93, psvr2-tweaks 79dac0df).
        const uint64_t n = ++CyberpunkVR_DebugEiNoEntry;
        if (n == 1 || (n % 1000) == 0) {
            log("[EI-DIAG] ExecuteIndirect with no hook entry -- DRAW DROPPED (list=%p vtable=%p count=%llu)",
                (void*)self, self ? *reinterpret_cast<void**>(self) : nullptr,
                (unsigned long long)n);
        }
        return;
    }
    // A NULL ARGUMENT BUFFER CANNOT BE EXECUTED: D3D12 dereferences it at +0xF0 and the process dies.
    // Measured on the psvr2-tweaks branch during a save load -- eight calls on one command list and
    // signature, maxCount=1, argOff marching 0,20,40..140, args=NULL on every one, vrcamNode=1 on
    // every one. So it is the SECOND EYE replaying a frame-graph node whose argument resource has not
    // been rebuilt yet; the main view never does it.
    //
    // Skipping is correct rather than merely defensive: an indirect draw with no arguments has nothing
    // to execute, so the choice is "skip" or "crash". We forward the caller's pointer and do not
    // supply it, so this is a guard and not a fix -- why the replay reaches an argument-less node
    // during a load is still open.
    if (!args) {
        const uint64_t n = ++CyberpunkVR_DebugEiNullArgs;
        if (n == 1 || (n % 500) == 0) {
            log("[EI-DIAG] skipped ExecuteIndirect with NULL argument buffer "
                "(list=%p sig=%p maxCount=%u argOff=%llu vrcamNode=%d count=%llu)",
                (void*)self, (void*)sig, maxCount, (unsigned long long)argOff,
                t_vrcam_node_active ? 1 : 0, (unsigned long long)n);
        }
        return;
    }
    e->indirect_original(self, sig, maxCount, args, argOff, cnt, cntOff);
    if (!CyberpunkVR_IndirectCensus || !g_exe_base) return;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    const uintptr_t work = t_current_node_work;
    if (work <= base) return;
    const uint32_t rva = static_cast<uint32_t>(work - base);
    const int v = t_vrcam_node_active ? 1 : 0;
    {
        std::lock_guard<std::mutex> lk(g_ind_mtx);
        uint32_t i = 0;
        for (; i < g_ind_n; ++i) if (g_ind[i].node_rva == rva && g_ind[i].sig == sig) break;
        if (i == g_ind_n) {
            if (g_ind_n >= g_ind.size()) return;
            g_ind[g_ind_n++] = { rva, sig, {0, 0} };
        }
        ++g_ind[i].hits[v];
    }
    indirect_report();
}

// ---- which node builds the colour-grading tables? -------------------------------------------
// The scanner's green tint is missing in the second eye, and the capture says why: the tonemap
// pass (PipelineState_777) is bound `3 x Texture3D<float3>` -- three 48^3 R11G11B10 tables --
// and at VRCAM's draw they hold a neutral colour cube while at MAIN's draw they hold the graded
// green one. Same shader, same three resources, rebuilt once per view by three 6x6x6 dispatches
// (48^3 / 8^3) in AsyncComputeDuringShadowmaps; only the per-view constants differ, and VRCAM's
// come out ungraded.
//
// The tables are a SHARED resource and the frame order is VRCAM then MAIN, so the fix is simply
// to let VRCAM skip its own build and sample the ones MAIN left. Colour grading is
// view-independent -- both eyes MUST have the same grade -- so that is the correct answer, not a
// workaround, and it costs the second eye one frame of grading latency.
//
// This probe exists only to name the node, so CyberpunkVR_NodeCutSet can target it without
// another rebuild. Cubic thread-group shapes are rare enough to be a clean filter.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_VolumeNodeProbe = 1;   // OFF: cubic-dispatch (volume/LUT) probe
static void volume_node_note(uint32_t rva, UINT n, bool vrcam) {
    struct Seen { uint32_t rva, n; uint8_t views; };
    static std::array<Seen, 16> s_seen{};
    static uint32_t s_n = 0;
    static std::mutex s_mtx;
    char line[600];
    int used = 0;
    {
        std::lock_guard<std::mutex> lk(s_mtx);
        uint32_t i = 0;
        for (; i < s_n; ++i) if (s_seen[i].rva == rva && s_seen[i].n == n) break;
        const uint8_t bit = vrcam ? 2 : 1;
        if (i < s_n) {
            if (s_seen[i].views & bit) return;          // already reported for this view
            s_seen[i].views |= bit;
        } else {
            if (s_n >= s_seen.size()) return;
            s_seen[s_n++] = { rva, n, bit };
        }
        line[0] = 0;
        for (uint32_t k = 0; k < s_n && used < static_cast<int>(sizeof(line)) - 40; ++k)
            used += snprintf(line + used, sizeof(line) - used, "%X:%ux%ux%u(%s%s) ",
                             s_seen[k].rva, s_seen[k].n, s_seen[k].n, s_seen[k].n,
                             (s_seen[k].views & 1) ? "M" : "-",
                             (s_seen[k].views & 2) ? "V" : "-");
    }
    log("[volnode] cubic dispatches, node:shape(views): %s", line);
}

 void STDMETHODCALLTYPE hk_Dispatch(ID3D12GraphicsCommandList* self,
        UINT x, UINT y, UINT z) {
    const CommandListVtableHook* e = command_list_hook_entry(self);
    if (!e || !e->dispatch_original) return;
    maybe_marker(self);
    e->dispatch_original(self, x, y, z);
    t_last_disp[0] = x; t_last_disp[1] = y; t_last_disp[2] = z;
    if (CyberpunkVR_DispatchCensus && g_exe_base) {
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        const uintptr_t work = t_current_node_work;
        if (work > base) {
            dispatch_census_note(static_cast<uint32_t>(work - base), x, y, z,
                                 t_vrcam_node_active);
            dispatch_census_report();
        }
    }
    if (CyberpunkVR_VolumeNodeProbe && g_exe_base && x == y && y == z && x >= 2 && x <= 16) {
        // Record UNATTRIBUTED ones too (rva 0). The 6x6x6 grading-volume builds live in the
        // AsyncComputeDuringShadowmaps list and never showed up here, and the question that
        // decides the fix is WHY: if they arrive with rva 0 then t_current_node_work -- which is
        // thread-local and set by Detour_NodeDispatch -- is simply not set on whatever thread
        // records that list, and no node-level hook can ever see them.
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        const uintptr_t work = t_current_node_work;
        volume_node_note(work > base ? static_cast<uint32_t>(work - base) : 0u, x,
                         t_vrcam_node_active);
    }
    if (CyberpunkVR_LightContent) light_content_report();
}

 void STDMETHODCALLTYPE hk_CopyBufferRegion(ID3D12GraphicsCommandList* self,
        ID3D12Resource* dst, UINT64 dst_off, ID3D12Resource* src, UINT64 src_off,
        UINT64 num_bytes) {
    const CommandListVtableHook* e = command_list_hook_entry(self);
    if (!e || !e->cbr_original) return;
    e->cbr_original(self, dst, dst_off, src, src_off, num_bytes);
    // ---- how many lights does each view actually get? ---------------------------------------
    // Everything measurable about the two views is identical -- same camera to the byte bar the
    // 6.4 cm IPD, same near/far, same FOV, same passes, same cull constants -- and yet a whole
    // class of lights never lights in VRCAM at any distance. So stop comparing inputs and
    // measure the OUTPUT: the light nodes upload their per-view light array through here, and
    // its byte volume is proportional to the number of lights that survived collection.
    // Fewer bytes for VRCAM = lights are lost during collection; equal bytes = they are all
    // there and the difference is downstream, in how they are applied.
    // Sizes and counts of the light uploads match bin for bin, and every lighting node now
    // provably dispatches for both views -- so the remaining difference can only be in the
    // CONTENT of the arrays. Snapshot the largest upload each view makes inside the light nodes
    // and compare them: same length and near-identical bytes means the lights really are the
    // same and the defect is in shading; a truncation or a run of zeros in VRCAM's is the answer.
    if (CyberpunkVR_LightContent && src && num_bytes >= 4096 && num_bytes <= LIGHT_SNAP_MAX &&
            g_exe_base) {
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        const uintptr_t work = t_current_node_work;
        if (work > base) {
            const uint32_t rva = static_cast<uint32_t>(work - base);
            if (rva == CLUSTERED_LIGHTS_CULL_RVA || rva == RENDER_LIGHT_BUFFERS_RVA)
                light_content_note(src, src_off, num_bytes, t_vrcam_node_active);
        }
    }
    if (CyberpunkVR_LightCensus && g_exe_base && num_bytes) {
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        const uintptr_t work = t_current_node_work;
        if (work > base) {
            const uint32_t rva = static_cast<uint32_t>(work - base);
            if (rva == CLUSTERED_LIGHTS_CULL_RVA || rva == RENDER_LIGHT_BUFFERS_RVA) {
                volatile LONG64* bytes = t_vrcam_node_active
                    ? reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugLightBytesVrcam)
                    : reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugLightBytesMain);
                volatile LONG64* count = t_vrcam_node_active
                    ? reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugLightUploadsVrcam)
                    : reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugLightUploadsMain);
                InterlockedExchangeAdd64(bytes, static_cast<LONG64>(num_bytes));
                InterlockedIncrement64(count);
                D3D12_RESOURCE_DESC dd{};
                const uint64_t dst_size =
                    (dst && mirror_get_resource_desc(dst, &dd)) ? dd.Width : 0;
                light_census_note(num_bytes, dst_size, dst, t_vrcam_node_active);
                light_census_report();
            }
        }
    }
    // First 848B CB upload after the vrcam tonemap 2-RT bind = the pass's constants
    // (observed: bind ev95006 -> upload ev95009, one per window).
    if (t_in_vrcam_2rt && t_2rt_cb_armed && dst && num_bytes == 848) {
        t_2rt_cb_armed = false;
        ID3D12Resource* prev = g_cb_res.exchange(dst, std::memory_order_acq_rel);
        if (prev != dst) {
            dst->AddRef();
            if (prev) prev->Release();
        }
        g_cb_off.store(dst_off, std::memory_order_release);
        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
            &CyberpunkVR_DebugCbCaptures));
    }
    // Every fill from a mappable source, remembered so a DEFAULT-heap buffer can still be read on
    // the CPU. Gated: it costs one GetGPUVirtualAddress per copy.
    if (CyberpunkVR_SightAxisProbe && dst && src && num_bytes >= 64) {
        if (const uint8_t* sp = upload_map_read(src))
            filled_note_guarded(dst, dst_off, num_bytes, sp + src_off);
    }
    // The per-frame constants the HUD composite reads as b0 -- 30 float4, and its first float is
    // the time that drives the HUD's scanline flicker. Exactly one 480-byte constant upload
    // happens per frame, which is what identifies it. Captured so our port can bind the SAME
    // buffer as a root CBV and read the SAME value: the flicker is a spatial pattern, so a phase
    // of our own would make the two eyes disagree pixel by pixel.
    if (dst && num_bytes == 480 && dst_off == 0) {
        ID3D12Resource* prev = g_frame_cb.load(std::memory_order_acquire);
        if (prev != dst) {
            dst->AddRef();
            g_frame_cb.store(dst, std::memory_order_release);
            if (prev) prev->Release();
            CyberpunkVR_DebugFrameCb = reinterpret_cast<uint64_t>(dst);
        }
    }
    // The composite's OWN constants (b6). Identified by CONTENT rather than by size: the capture
    // showed a single 512-byte upload, but live at 2560x2560 there is none, and keying on the
    // size simply never matched. The upload heap is CPU-visible, so the source bytes can be read
    // here and checked -- register 16 zw is the composite's target size, which nothing else
    // carries. That also makes the binding correct at any resolution, which is the whole point:
    // the constants read out of a 1920x1080 capture are not the ones a 2560 square uses.
    if (dst && src && num_bytes >= 272 && num_bytes <= 4096 &&
        !g_hud_cb_from_ring.load(std::memory_order_acquire)) {
        uint64_t w = 0, h = 0;
        {
            std::lock_guard<std::mutex> lk(g_hud_snap_mtx);
            w = g_hud_snap_desc.Width; h = g_hud_snap_desc.Height;
        }
        float curve[2] = {0.0f, 0.0f};
        if (w && h && hud_cb_content_matches(upload_map_read(src), src_off,
                                             (float)w, (float)h, curve)) {
            ID3D12Resource* prev = g_hud_cb.load(std::memory_order_acquire);
            if (prev != dst) {
                dst->AddRef();
                g_hud_cb.store(dst, std::memory_order_release);
                if (prev) prev->Release();
                CyberpunkVR_DebugHudCb = reinterpret_cast<uint64_t>(dst);
                log("[hud] composite constants captured: dst=%p bytes=%llu target=%llux%llu "
                    "curvature=(%.6f, %.6f)", dst, (unsigned long long)num_bytes,
                    (unsigned long long)w, (unsigned long long)h, curve[0], curve[1]);
            }
        }
    }
}

}  // namespace detail
}  // namespace cvr
