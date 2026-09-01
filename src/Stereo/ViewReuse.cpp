// ViewReuse -- the five things the second eye reuses instead of recomputing.
//
// Sky, clouds, distant shadows, local shadows and global illumination. Each of these is expensive and
// each is, to a very good approximation, the same for two eyes 65 mm apart -- so the detours here
// either let the second view read the first view's result or skip its producer outright.
//
// Two rules this family has already cost real bugs to learn, both recorded at their use sites:
//
//   * Skipping a producer while leaving its feature bit SET is deliberate: downstream consumers must
//     still run, reading what the first view produced. Clearing the bit instead makes the second eye
//     lose the effect entirely.
//   * Only the primary view's cloud wind offsets advance, so the second view's clouds stand still
//     unless the offsets are mirrored per frame. That is what the cloud constant-buffer machinery
//     below is for, and it is why it copies a snapshot rather than sharing a pointer.

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
#include "Stereo/DetourRegistry.hpp"

namespace cvr {
namespace detail {

void __fastcall Detour_SkyWork(void* a1, void* a2) {
    bool vrcam = false;
    if (a2) {
        __try {
            const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
                reinterpret_cast<uint8_t*>(a2) + 0x18);
            vrcam = ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
        } __except (EXCEPTION_EXECUTE_HANDLER) { vrcam = false; }
    }
    if (vrcam) {
        if (CyberpunkVR_SkyReuseMode == 1) {
            InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugSkySkipHits));
            return;
        }
    } else {
        InterlockedIncrement64(
            reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugSkyMainHits));
    }
    g_orig_sky_work(a1, a2);
}

using DistantWorkFn = void(__fastcall*)(void*, void*, void*);
static DistantWorkFn g_orig_distant_render  = nullptr;
static DistantWorkFn g_orig_distant_prepare = nullptr;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugDistantSkipHits = 0;

// SEH-only helper (no C++ objects)  view ctx = *(a2+0x18), key @ctx+0x28.
// Only skip when distant-reuse mode is active (1); mode 0 leaves distant to the
// engine's own bit-11-clear no-op so the A/B baseline is the vanilla path.
static bool distant_is_vrcam(void* a2) {
    if (CyberpunkVR_DistantReuseMode != 1 || !a2) return false;
    __try {
        uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
            reinterpret_cast<uint8_t*>(a2) + 0x18);
        return ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

static void __fastcall Detour_DistantRender(void* a1, void* a2, void* a3) {
    if (distant_is_vrcam(a2)) { ++CyberpunkVR_DebugDistantSkipHits; return; }
    g_orig_distant_render(a1, a2, a3);
}
static void __fastcall Detour_DistantPrepare(void* a1, void* a2, void* a3) {
    if (distant_is_vrcam(a2)) { ++CyberpunkVR_DebugDistantSkipHits; return; }
    g_orig_distant_prepare(a1, a2, a3);
}

// --- Volumetric clouds: hand VRCAM MAIN's cloud constants ----------------------------
// MEASURED live in both views (x64dbg, breakpoint inside CRenderNode_RenderVolumetricClouds
// sub_14061B5B4 right after it resolves its view object): the cloud parameter block at
// viewData+0x550 is byte-identical between MAIN and VRCAM for its first 0x9C bytes, and then
// differs in EXACTLY six floats at +0x9C..+0xB3 -- the wind-scroll offsets of the three
// cloud-noise octaves. MAIN carried the offsets accumulated over the session,
//     (-29.50, -255.00)   (-39.29, -412.05)   (-49.58, -433.98)
// while VRCAM's were ZERO and stayed zero: the engine only ever advances the primary view's.
// sub_140784654 multiplies those six by 0.1 into the cloud constant buffer as the UV offsets
// of the three noise layers, so the two eyes sample the cloud field in completely different
// places -- different shapes, a cloud present in one eye and absent in the other, and no
// convergence while standing still (which is what ruled temporal accumulation out).
//
// It has to be done per frame: the view object comes from a pool and is a different address
// every frame (proven -- a one-shot poke of the six floats was gone by the next hit), and the
// fresh one always arrives zeroed.
//
// ONLY those six values are mirrored. Copying the whole buffer was tried first and is WRONG:
// it makes VRCAM's clouds disappear entirely (tested live, both variants). The buffer also
// carries fields that are genuinely per-view -- +140/+160/+164 come out of the frame's
// resource resolve (sub_1401F3D20), i.e. descriptor indices for TRANSIENT targets, so MAIN's
// point at memory that has been aliased to something else by the time VRCAM's pass runs.
// +168/+172 are that view's jitter and +144/+148 its resolved cloud-target size.
//
// Blast radius is clouds and nothing else: sub_140784654's only caller in the 169-node graph
// is the volumetric-clouds node, and viewData+0x430/+0x550 are read by no other node either.
// Size is exact, not assumed -- the caller follows the fill with sub_1401EE3CC(0xC0, Src).
constexpr size_t    CLOUD_CB_BYTES    = 192;        // 0xC0, per the upload right after it
using CloudCbFn = __int64(__fastcall*)(__int64, __int64, __int64, __int64, __int64,
                                       int*, int, int);
static CloudCbFn g_orig_cloud_cb = nullptr;
// 3 = mirror ONLY the three noise-layer offsets -- the measured difference  [default, proven]
// 0 = off (engine's own per-view constants, i.e. the broken-stereo baseline)
// 1 = mirror MAIN's buffer, keep VRCAM's own view size + jitter   -- kills VRCAM's clouds
// 2 = mirror MAIN's buffer verbatim                               -- kills VRCAM's clouds
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CloudCbMode = 3;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCloudCbMain  = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCloudCbVrcam = 0;
static std::mutex g_cloud_cb_mtx;
static uint8_t    g_cloud_cb_main[CLOUD_CB_BYTES];
static std::atomic<bool> g_cloud_cb_have{false};

// SEH-only helpers below: they touch engine-owned memory on job threads, and __try cannot
// share a function with C++ object unwinding (C2712), so the locking stays out of them.
bool cloud_cb_raw_copy(void* dst, const void* src, size_t n) {
    __try { memcpy(dst, src, n); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

// AS MUCH AS IS ACTUALLY THERE. The viewData capture was all-or-nothing on 0xFF0 bytes and it
// never landed -- no [viewData] line has ever been logged, while the graph-context diff beside
// it reports fine. 0xFF0 came from the HUD reversing and evidently overruns the object on this
// path. Copy in 64-byte steps and return how far we got, so a short object still gets diffed.
static size_t raw_copy_upto(void* dst, const void* src, size_t n) {
    size_t done = 0;
    while (done < n) {
        const size_t step = (n - done) < 64 ? (n - done) : 64;
        if (!cloud_cb_raw_copy(static_cast<uint8_t*>(dst) + done,
                               static_cast<const uint8_t*>(src) + done, step)) break;
        done += step;
    }
    return done;
}

// a3 is the node work-context: view ctx = *(a3+0x18), key @ ctx+0x28. MAIN is 0, VRCAM is
// g_vrcam_ctx_key; every other view (shadow, reflection, ...) is left entirely alone.
static int cloud_cb_view(__int64 a3) {
    if (!a3) return -1;
    __try {
        uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
            reinterpret_cast<uint8_t*>(a3) + 0x18);
        if (!ctx) return -1;
        const uint64_t key = *reinterpret_cast<uint64_t*>(ctx + 0x28);
        if (key == 0) return 0;
        if (key == g_vrcam_ctx_key) return 1;
        return -1;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return -1; }
}

static void cloud_cb_capture_main(__int64 cb) {
    uint8_t tmp[CLOUD_CB_BYTES];
    if (!cb || !cloud_cb_raw_copy(tmp, reinterpret_cast<const void*>(cb), CLOUD_CB_BYTES))
        return;
    {
        std::lock_guard<std::mutex> lk(g_cloud_cb_mtx);
        memcpy(g_cloud_cb_main, tmp, CLOUD_CB_BYTES);
    }
    g_cloud_cb_have.store(true, std::memory_order_release);
    InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCloudCbMain));
}

// ONLY 0x40. The buffer's other differing fields are per-view by construction and the mode-1
// path below already says so: +152 is this view's width/height and +168 its jitter, both
// deliberately preserved there. 0xA8 IS +168, so mirroring it -- which the first version of
// this did -- hands VRCAM MAIN's jitter. 0x90 (+144) is the resolved cloud-target size, which
// is why it reads {M 1 | V 512}. +0x8C/+0xA0/+0xA4 are transient descriptor indices and
// mirroring the buffer wholesale is already known to kill VRCAM's clouds outright.
//   bit 0  0x40..0x4F -- the WHOLE float4, not just the first dword. The diff prints where
//          a run starts, never how long it is, and 4 bytes was the same off-by-one that cost
//          five rounds on viewData. The raymarch reads _40_m0[4].w -- byte 0x4C -- as the
//          light-intensity multiplier: `_412 = _40_m0[4].w * _30_m0[6].x` for each channel,
//          i.e. a flat scale on the cloud colour. That is the shape of "always lighter".
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CloudCbExtra = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCloudCbExtra = 0;

// Report-only diff of the whole 192 bytes, taken BEFORE anything is written -- which is why 0x50
// reads as a hole here: the wind mirror fills it a few lines further down.
static void block_diff_log(const char* tag, const uint8_t* refb, const uint8_t* curb,
                           size_t bytes);
static void cloud_cb_diff_vrcam(__int64 cb) {
    static uint64_t s_last = 0;
    if (!cb || !g_cloud_cb_have.load(std::memory_order_acquire)) return;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 15000) return;
    uint8_t cur[CLOUD_CB_BYTES], ref[CLOUD_CB_BYTES];
    if (!cloud_cb_raw_copy(cur, reinterpret_cast<const void*>(cb), CLOUD_CB_BYTES)) return;
    {
        std::lock_guard<std::mutex> lk(g_cloud_cb_mtx);
        memcpy(ref, g_cloud_cb_main, CLOUD_CB_BYTES);
    }
    s_last = now;
    block_diff_log("cloudCB", ref, cur, CLOUD_CB_BYTES);
}


static void cloud_cb_apply_vrcam(__int64 cb) {
    // No MAIN snapshot yet (first frames, or MAIN's clouds gated off) -> leave the engine's
    // own constants alone. An optional input must degrade, never blank the pass.
    if (!cb || !g_cloud_cb_have.load(std::memory_order_acquire)) return;
    const uint32_t mode = CyberpunkVR_CloudCbMode;
    uint8_t tmp[CLOUD_CB_BYTES];
    {
        std::lock_guard<std::mutex> lk(g_cloud_cb_mtx);
        memcpy(tmp, g_cloud_cb_main, CLOUD_CB_BYTES);
    }
    if (mode == 3) {
        if (CyberpunkVR_CloudCbExtra & 1)
            cloud_cb_raw_copy(reinterpret_cast<uint8_t*>(cb) + 0x40, tmp + 0x40, 16);
        if (CyberpunkVR_CloudCbExtra)
            InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCloudCbExtra));
        // The three noise-layer offsets land at CB+80..+103 (a5+156..+176, each x0.1).
        if (cloud_cb_raw_copy(reinterpret_cast<uint8_t*>(cb) + 80, tmp + 80, 24))
            InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCloudCbVrcam));
        return;
    }
    if (mode == 1) {
        uint8_t keep[16];
        if (cloud_cb_raw_copy(keep, reinterpret_cast<const uint8_t*>(cb) + 152, 8) &&
            cloud_cb_raw_copy(keep + 8, reinterpret_cast<const uint8_t*>(cb) + 168, 8)) {
            memcpy(tmp + 152, keep, 8);       // this view's width/height
            memcpy(tmp + 168, keep + 8, 8);   // this view's jitter, in its own NDC
        }
    }
    if (cloud_cb_raw_copy(reinterpret_cast<void*>(cb), tmp, CLOUD_CB_BYTES))
        InterlockedIncrement64(
            reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCloudCbVrcam));
}

// --- what ELSE of the view block does the RTT view not get? --------------------------------
// The cloud wind offsets turned out to be one instance of a pattern: sub-blocks of viewData
// that the engine only ever fills for the primary view. ScreenSpaceRain is measurably another
// -- it runs for VRCAM but early-outs, because its gate is
//     sub_1401ED930(wc) + 0xAB0, floats at +52 / +72 / +76, at least one > 0
// and those are zero for VRCAM (audit: the node is 119x cheaper there). So rather than chase
// them one symptom at a time, diff the WHOLE view block once and let it name every hole.
// Runs off the cloud hook because that already has viewData in hand for both views
// (a4 == viewData + 0x430), so no extra engine call and no new hook.
// MODE 2 for one run. Mode 1 lists only MAIN-set/VRCAM-zero holes, and the night difference is
// not a hole -- both views have values, they are simply not the same values. The user reports
// that VRCAM does not follow the dusk-to-dawn transition at all, which is a TIME failure: the
// weather/time-of-day blend reaches MAIN's view block and not the second one. Mode 2 prints
// every differing run, so the extent of that block gets named instead of guessed at.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ViewDataDiff = 2;   // OFF: 33 MB and 16693 lines per session
constexpr size_t VIEWDATA_BYTES = 0xFF0;      // the view object's size, from the HUD reversing
static uint8_t g_vd_main[VIEWDATA_BYTES];
static std::atomic<bool> g_vd_have{false};

// The MAIN-set / VRCAM-zero ranges the diff reported live, with what the static reverse says
// reads them (traced by following sub_1401ED930's result through every decompiled node body):
//   0x150, 0x350        -> ScreenSpaceRain's block (base 0x334 / 0x4C)
//   0x168, 0x1A0, 0x1DC, 0x268 -> ONE block based at 0x168, read by CompositionPostProcess,
//        DrawComposition and GenerateTonemappingLUT. This is the same +0x168 those three nodes
//        gate on and that crashed twice when faked -- it is an output-resource set, not data.
//   0x5EC               -> the cloud wind offsets (already fixed via the cloud CB)
//   0xAF8               -> ScreenSpaceRain's wetness gate -- PROVEN: filling it brings the
//                          puddles back
//   0xF88, 0xFCC, 0xFE0 -> one block based at 0xF80, read by CompositionPostProcess and
//        RenderDebugSystems
struct ViewDataHole { uint16_t off, len; const char* note; };
static const ViewDataHole kViewDataHoles[] = {
    { 0x150,  4, "rain block" },      // bit 0
    { 0x168, 16, "composition out" }, // bit 1  -- DANGEROUS: resource set, see above
    { 0x1A0,  4, "composition out" }, // bit 2
    { 0x1DC,  8, "composition out" }, // bit 3
    { 0x268,  4, "composition out" }, // bit 4
    { 0x350, 12, "rain block" },      // bit 5
    { 0x5EC, 24, "cloud wind" },      // bit 6  -- already handled via the cloud CB
    { 0xAF8,  8, "rain wetness" },    // bit 7  -- PROVEN
    { 0xF88,  8, "composition/debug" },// bit 8
    { 0xFCC,  4, "composition/debug" },// bit 9
    { 0xFE0, 12, "composition/debug" },// bit 10
    // FOUND AT NIGHT, 2026-07-31. The table above was built in daylight and these two never
    // appeared in it. Both are plain floats -- no pointer anywhere near them -- so they are in
    // the safe class, unlike the 0x168 resource set.
    //   0x1CC  {2.4278, 118241.1, 118241.1}
    //   0x4A0  {400000.0}   <- a lone large float, and VRCAM has ZERO there
    // 400000 with nothing on the other side is the shape of a far/streaming distance, and a far
    // distance of zero is precisely "the second eye shows nothing in the distance". Read as a
    // squared distance it is 632 m; as centimetres, 4 km. Either is a plausible city far plane.
    { 0x1CC, 12, "night pair" },      // bit 11
    { 0x4A0,  4, "no reader" },       // bit 12  -- 400000.0, but nothing reads it
};

// Shared by the viewData and graph-context diffs: report the dword runs that differ, starring
// the ones where MAIN has a value and the RTT view has nothing -- those are the sub-blocks the
// engine never fills for it, which is the whole class of bug this chases.
static void block_diff_log(const char* tag, const uint8_t* refb, const uint8_t* curb,
                           size_t bytes) {
    const uint32_t* m = reinterpret_cast<const uint32_t*>(refb);
    const uint32_t* v = reinterpret_cast<const uint32_t*>(curb);
    const size_t nd = bytes / 4;
    char line[4000];
    int used = 0, runs = 0, holes = 0;
    line[0] = '\0';
    for (size_t i = 0; i < nd; ) {
        if (m[i] == v[i]) { ++i; continue; }
        const size_t s = i;
        while (i < nd && m[i] != v[i]) ++i;
        ++runs;
        bool vr_zero = true, main_set = false;
        for (size_t k = s; k < i; ++k) { if (v[k]) vr_zero = false; if (m[k]) main_set = true; }
        const bool hole = vr_zero && main_set;
        if (hole) ++holes;
        // Printing only the holes was a real blind spot: it hides every field where the second
        // view has its OWN non-zero value, which is precisely the shape of the grading-LUT
        // selection (MAIN picks Resource_345, VRCAM picks Resource_2123) and of any other
        // per-view setting that is chosen rather than left empty. Mode 2 prints them all.
        const bool want = (CyberpunkVR_ViewDataDiff >= 2) ? true : hole;
        // BOTH SIDES, and never truncated. Mode 2 used to print only MAIN's dwords into one
        // 1700-char line, which answered "these ranges differ" but not the question that matters
        // for a stale-environment theory: does the SECOND view's value move at all between two
        // samples? Print m|v for the first dword of every run and flush whenever the line fills,
        // so a night capture and a dawn capture can be held side by side.
        //
        // AND PRINT THE RUN LENGTH, which used to be invisible and cost a full round trip. Reading
        // "720{3F800000|457A0000}" as "the field at 0x720 differs" is wrong: that is the FIRST dword
        // of a run that may be several dwords long. A mirror was then written covering four bytes,
        // the log dutifully stopped showing 0x720 -- and the next run simply started at 0x724, whose
        // value (8.0 against 5000.0) was the other half of the same pair and the half that mattered.
        // The length is now in the output as *N, so a range can be sized from the log instead of
        // guessed. Same trap as the fixed-size tables this file warns about: the diagnostic was
        // narrower than the thing it measured.
        if (want) {
            if (used > static_cast<int>(sizeof(line)) - 48) {
                log("[vdiff] %s M|V cont: %s", tag, line);
                used = 0; line[0] = 0;
            }
            used += snprintf(line + used, sizeof(line) - used, "%X*%u{%08X|%08X}%s ",
                             static_cast<unsigned>(s * 4), static_cast<unsigned>(i - s),
                             m[s], v[s], hole ? "H" : "");
        }
    }
    if (CyberpunkVR_ViewDataDiff >= 2) {
        log("[vdiff] %s M|V %d runs (%d holes, H marked) END: %s",
            tag, runs, holes, used ? line : "(none)");
    } else {
        log("[vdiff] %s MAIN vs VRCAM: %d differing runs, %d MAIN-set/VRCAM-zero: %s",
            tag, runs, holes, holes ? line : "(none)");
    }
}

// The graph CONTEXT is the other place per-view state lives, and it is where the remaining
// symptom has to be: the viewData holes are all accounted for above and none of them is
// lighting. Same treatment -- capture MAIN's, diff VRCAM's, report the holes.
constexpr size_t CTX_BYTES = 0x2000;      // covers everything we have ever seen used (0x1E10+)
static uint8_t g_ctx_main[CTX_BYTES];
static std::atomic<bool> g_ctx_have{false};

static void ctx_capture_main(__int64 ctx) {
    uint8_t tmp[CTX_BYTES];
    if (!ctx || !cloud_cb_raw_copy(tmp, reinterpret_cast<const void*>(ctx), CTX_BYTES)) return;
    {
        std::lock_guard<std::mutex> lk(g_cloud_cb_mtx);
        memcpy(g_ctx_main, tmp, CTX_BYTES);
    }
    g_ctx_have.store(true, std::memory_order_release);
}

static void ctx_diff_vrcam(__int64 ctx) {
    if (!ctx || !g_ctx_have.load(std::memory_order_acquire)) return;
    uint8_t cur[CTX_BYTES], ref[CTX_BYTES];
    if (!cloud_cb_raw_copy(cur, reinterpret_cast<const void*>(ctx), CTX_BYTES)) return;
    {
        std::lock_guard<std::mutex> lk(g_cloud_cb_mtx);
        memcpy(ref, g_ctx_main, CTX_BYTES);
    }
    block_diff_log("graphCtx", ref, cur, CTX_BYTES);
    // ALL the frame-graph feature words, not just f0/f1. sub_14023AF5C indexes them as
    //     *(ctx + 8*(bit >> 6) + 6096)
    // so the bitset runs well past the two words this project has always logged and forced. A
    // bit MAIN has and VRCAM lacks in word 2 or beyond would gate a node exactly like bit 25
    // gates the clouds -- and the whole-context diff above cannot show it, because that diff
    // only flags ranges where VRCAM is ZERO, and a word that is merely missing one bit is not.
    char fl[1200];
    int fu = 0;
    fl[0] = 0;
    for (int w = 0; w < 24 && fu < static_cast<int>(sizeof(fl)) - 64; ++w) {
        uint64_t m = 0, v = 0;
        memcpy(&m, ref + 6096 + w * 8, 8);
        memcpy(&v, cur + 6096 + w * 8, 8);
        if (m == v) continue;
        fu += snprintf(fl + fu, sizeof(fl) - fu, "f%d m=%016llX v=%016llX miss=%016llX ", w,
                       (unsigned long long)m, (unsigned long long)v,
                       (unsigned long long)(m & ~v));
    }
    log("[fgflags-all] words differing between the views (miss = MAIN has, VRCAM lacks): %s",
        fu ? fl : "(none)");
}

static std::atomic<size_t> g_vd_len{0};
static void viewdata_capture_main(__int64 vd) {
    uint8_t tmp[VIEWDATA_BYTES];
    if (!vd) return;
    const size_t got = raw_copy_upto(tmp, reinterpret_cast<const void*>(vd), VIEWDATA_BYTES);
    if (got < 256) return;
    {
        std::lock_guard<std::mutex> lk(g_cloud_cb_mtx);
        memcpy(g_vd_main, tmp, got);
    }
    g_vd_len.store(got, std::memory_order_release);
    g_vd_have.store(true, std::memory_order_release);
}

// Logged on a timer, not once, so the scene can be changed (step into the rain, walk up to the
// stalls) and a fresh answer read out without a restart.
static void viewdata_diff_vrcam(__int64 vd) {
    static uint64_t s_last = 0;
    if (!vd || !g_vd_have.load(std::memory_order_acquire)) return;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 15000) return;
    uint8_t cur[VIEWDATA_BYTES], ref[VIEWDATA_BYTES];
    size_t n = raw_copy_upto(cur, reinterpret_cast<const void*>(vd), VIEWDATA_BYTES);
    const size_t mainLen = g_vd_len.load(std::memory_order_acquire);
    if (n > mainLen) n = mainLen;
    n &= ~size_t(3);
    if (n < 256) { log("[viewData] unreadable: got %zu bytes of %u", n, (unsigned)VIEWDATA_BYTES);
                   s_last = now; return; }
    {
        std::lock_guard<std::mutex> lk(g_cloud_cb_mtx);
        memcpy(ref, g_vd_main, n);
    }
    s_last = now;
    block_diff_log("viewData", ref, cur, n);
    // THE FOG-OVERLAY INPUTS, BY NAME. sub_14061F9E0 (RenderFogOverlay) reads a float block at
    // viewData+0x8C0 and gates the whole distant-fog path on viewData+0x920 > 0.0:
    //     if ((f0 & 0x800000) == 0 && !sub_1401E4B60(ctx+20) || *(float*)(vd+2336) <= 0.0) v6 = 0;
    //     if ((f0 & 0x1000000) || (f0 & 0x4000000) || v6) { ...fog... }
    //     v19 = (float*)(vd + 2240);  v21 = *(vd+2332) * *(float*)(vd+2328);
    // These are not holes -- both views have values there -- so the hole report above cannot see
    // them. A fog that is too strong hides the far city AND washes the stars out of the sky,
    // which is one cause for both reported symptoms, so print the two sides and compare.
    if (n >= 0x928) {
        char fl[700];
        int u = 0;
        fl[0] = 0;
        static const uint16_t kFogOff[] = { 0x8C0, 0x8C4, 0x8C8, 0x8CC, 0x8D0, 0x8D4,
                                            0x918, 0x91C, 0x920, 0x924 };
        for (size_t k = 0; k < sizeof(kFogOff) / sizeof(kFogOff[0]); ++k) {
            float fm = 0.0f, fv = 0.0f;
            memcpy(&fm, ref + kFogOff[k], 4);
            memcpy(&fv, cur + kFogOff[k], 4);
            if (u < static_cast<int>(sizeof(fl)) - 64)
                u += snprintf(fl + u, sizeof(fl) - u, "%X{M %.4g|V %.4g}%s ",
                              kFogOff[k], fm, fv, (fm == fv) ? "" : " <<");
        }
        log("[fog] RenderFogOverlay inputs, MAIN|VRCAM (<< marks a difference): %s", fl);
    }
    // What is actually IN MAIN's holes decides whether a hole is safe to fill: a float is data,
    // a user-space address is a resource handle VRCAM does not own (viewData+0x168 is exactly
    // that -- the composition output set that crashed twice when it was faked).
    char vals[900];
    int used = 0;
    vals[0] = '\0';
    for (size_t i = 0; i < sizeof(kViewDataHoles) / sizeof(kViewDataHoles[0]); ++i) {
        const ViewDataHole& h = kViewDataHoles[i];
        float f0 = 0, f1 = 0;
        uint64_t q = 0;
        memcpy(&f0, ref + h.off, 4);
        if (h.len >= 8) { memcpy(&f1, ref + h.off + 4, 4); memcpy(&q, ref + h.off, 8); }
        if (used < static_cast<int>(sizeof(vals)) - 80)
            used += snprintf(vals + used, sizeof(vals) - used,
                             "b%zu@%X{%.4g,%.4g%s} ", i, h.off, f0, f1,
                             (h.len >= 8 && q >= 0x10000000000ull && q < 0x7FFFFFFFFFFFull)
                                 ? ",PTR" : "");
    }
    log("[vdiff] MAIN's values in the holes: %s", vals);
}

// --- LET THE ENGINE COMPUTE IT, instead of copying MAIN's bytes ---------------------------------
//
// The byte mirror can only ever move values that are safe to move: of the 53 runs that differ between
// the views, 14 hold pointers or refcounted handles and 8 more corrupt an engine container when
// written (proven -- the same fault address as that evening's first crash). Those are exactly the
// ones a copy cannot deliver.
//
// WHAT IT ACTUALLY IS, read out of the decompile rather than assumed. sub_1403BAFFC(stack, source)
// applies the stack of ACTIVE ENVIRONMENT OVERRIDES onto one view:
//
//     EnterCriticalSection(stack + 136)
//     for each entry, stride 64:
//         weight = *(float*)(entry + 4)              // its blend phase
//         if it has faded out          -> THE ENTRY IS ERASED FROM THE LIST
//         if weight == 1.0             -> state 1 -> 0, fully blended in
//         else apply it to `source`    (sub_1403BB0A8, which also reads source+0xF88)
//     LeaveCriticalSection
//
// That is where the game puts focus_mode.envparam, which is why running it for our source is what
// brought the scanner's green tint back into the second eye -- confirmed on the picture.
//
// CORRECTION, and it was mine: the note that first shipped here claimed this writes viewData+0x798,
// the field the second eye's darkness was bisected down to. It does not. Its only caller,
// sub_14029E1BC, writes the view's own fields (+460, +464, +468, +820, +3919) and calls
// sub_14029E2A0 BEFORE reaching this line, and we run only this line. sub_14029E2A0 contains zero
// references to +0x798. So this mechanism delivers the environment stack and nothing else -- the tint
// is the whole effect, not a side effect, and brightness is still open.
//
// THE HAZARD, stated because it is specific and not hypothetical: this MUTATES SHARED STATE. It
// erases faded entries from a list held under a critical section, and we call it a second time per
// frame from another thread. Evicting an entry before MAIN's own call reaches it is the same class of
// fault as the one-shot flag in the composition assign sub_140201A68, which is what made MAIN judder
// and the HUD flicker. Its params pointer is a persistent heap object (measured identical across
// frames), captured here from MAIN's own call.
//
// This is the same shape as every fix in this port that worked: run the mechanism the working view
// uses, rather than transcribing its results.
using ViewParamsApplyFn = void (__fastcall*)(uintptr_t, __int64);
using ViewParamsEntryFn = char (__fastcall*)(__int64, __int64, __int64);
static ViewParamsEntryFn g_view_params_entry = nullptr;
static ViewParamsApplyFn g_orig_view_params = nullptr;
static std::atomic<uintptr_t> g_view_params{0};
// DEFAULT 1 ON 2026-08-29, ON THE PICTURE. Reported by the user the first time it was switched on:
// the scanner's green tint is back in the second eye. That isolates a finding this repository had
// only ever recorded collectively -- the tint was first attributed to lending root parameter 5 of the
// LUT build, then to "comp_lend plus viewdata_fix_mask plus run_view_params" together. It is this
// alone: comp_lend is off, LutTableLend does not exist in this build, and the tint is there.
//
// Which matters beyond the tint, because comp_lend is the mechanism that crashed twice inside
// DrawComposition today. It is not needed for this, and nothing else has asked for it since.
//
// Measured alongside: runs=2626 against MAIN's params seen 3327 over the same window, both detours
// registered, no crash dump in forty minutes of play.
// DEFAULT 2, NOT 1. Mode 1 is the engine's whole pass and it erases entries from a list MAIN shares;
// the 0x88 driver fault returned 229 seconds into the one session it ran in. Mode 2 delivers the same
// thing -- the scanner's tint was measured with mode 1 and mode 2 applies the identical per-entry
// function -- with the erase gone. Shipping the mode with a named hazard as the default would be
// choosing the worse of two that do the same job.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_RunViewParams = 2;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewParamsRuns = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewParamsSeen = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewParamsEntries = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewParamsBusy = 0;
// WALKED versus APPLIED, and why the difference. Measured 2026-08-30: across a whole session the
// ratio of entries applied to runs is exactly 2.00 for minutes at a time and then exactly 3.00 for
// the rest, with lock busy at 0 and runs never stalling -- and the braindance tint is present in
// the 3.00 window and absent in the 2.00 one. So the pass runs in both and is one override short
// in the first, and the only question left is whether that entry is missing from the stack we read
// or present and dropped by one of the two skips below.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewParamsWalked   = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewParamsSkipNoObj = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewParamsSkipErase = 0;

static void __fastcall Detour_ViewParamsApply(uintptr_t a1, __int64 a2) {
    if (!g_view_params_entry && g_exe_base)
        g_view_params_entry = reinterpret_cast<ViewParamsEntryFn>(
            reinterpret_cast<uint8_t*>(g_exe_base) + VIEW_PARAMS_ENTRY_RVA);
    if (a1 > 0x10000) {
        g_view_params.store(a1, std::memory_order_release);
        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugViewParamsSeen));
    }
    g_orig_view_params(a1, a2);
}
CVR_DETOUR("[viewparams] per-view parameter apply sub_1403BAFFC", VIEW_PARAMS_APPLY_RVA,
           Detour_ViewParamsApply, g_orig_view_params)

// Runs the engine's producer on our own source, inside our preparer's call, on its thread, before the
// copy that carries the struct into the block. Calls the ORIGINAL, so the capture above cannot see its
// own work and the params pointer is never disturbed.
// MODE 2: THE SAME APPLY, WITHOUT THE ERASE.
//
// The destructive half is separable, which is the whole point. sub_1403BAFFC is a loop that calls
// sub_1403BB0A8 per entry and ERASES the entry when it answers 1; the apply itself is the callee. So
// the list is walked here instead, the callee is called, and nothing is ever removed.
//
// The structure, read off the outer loop: `stack` holds a bucket count at +128 and a CRITICAL_SECTION
// at +136, buckets are 16 bytes from `stack` itself, each carrying its array base at +0 and its entry
// count at +12; entries are 64 bytes, the override object sits at +16 and the record the apply takes
// at +24.
//
// Two things make this safe where the whole pass was not:
//
//   * nothing is erased. sub_1403BB0A8 still mutates the ENTRY -- state 2 -> 0 on a faded override,
//     1 -> 0 on one that has finished blending in -- but both are idempotent transitions MAIN's own
//     call makes anyway. Removing a list element from a second thread is not.
//   * an entry the engine WOULD erase is skipped rather than applied. Applying it would zero its
//     weight and leave its state at 0, and MAIN's own pass only erases at state 2 -- so the dead
//     entry would sit in the list forever. The test is the same one the callee opens with.
//
// TryEnterCriticalSection, never Enter: this runs inside the RTT preparer, and if the engine is
// already holding that lock somewhere up our own call stack, blocking would hang the game. A frame
// skipped costs one frame of a fade; a deadlock costs the session.
static void vd_apply_params_no_erase(uintptr_t stack, __int64 src) {
    if (!g_view_params_entry) return;
    CRITICAL_SECTION* cs = reinterpret_cast<CRITICAL_SECTION*>(stack + 136);
    if (!TryEnterCriticalSection(cs)) {
        InterlockedIncrement64(
            reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugViewParamsBusy));
        return;
    }
    __try {
        uint32_t buckets = 0;
        memcpy(&buckets, reinterpret_cast<const void*>(stack + 128), 4);
        if (buckets > 4096) buckets = 0;                 // a count this large is not a count
        for (uint32_t b = 0; b < buckets; ++b) {
            const uintptr_t bucket = stack + 16ull * b;
            uintptr_t base = 0;
            uint32_t  n = 0;
            memcpy(&base, reinterpret_cast<const void*>(bucket), 8);
            memcpy(&n, reinterpret_cast<const void*>(bucket + 12), 4);
            if (!base || n > 4096) continue;
            for (uint32_t i = 0; i < n; ++i) {
                const uintptr_t slot = base + 64ull * i;
                uintptr_t obj = 0;
                memcpy(&obj, reinterpret_cast<const void*>(slot + 16), 8);
                InterlockedIncrement64(
                    reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugViewParamsWalked));
                if (!obj) {                              // the pass would erase this
                    InterlockedIncrement64(
                        reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugViewParamsSkipNoObj));
                    continue;
                }
                const uintptr_t rec = slot + 24;
                float w0 = 0.0f, w = 0.0f;
                int32_t state = 0;
                memcpy(&w0, reinterpret_cast<const void*>(rec), 4);
                memcpy(&w, reinterpret_cast<const void*>(rec + 4), 4);
                memcpy(&state, reinterpret_cast<const void*>(rec + 12), 4);
                if ((w <= 0.0f || w0 == 0.0f) && state == 2) {           // it would erase this too
                    InterlockedIncrement64(
                        reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugViewParamsSkipErase));
                    continue;
                }
                g_view_params_entry(static_cast<__int64>(rec), src, static_cast<__int64>(obj));
                InterlockedIncrement64(
                    reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugViewParamsEntries));
            }
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    LeaveCriticalSection(cs);
}

// 1 = the engine's whole pass, which also ERASES faded entries from a list MAIN shares. That is what
//     shipped first, and the 0x88 driver fault returned 229 seconds into the session it ran in.
// 2 = the walk above: same apply, same lock, nothing removed.
static void vd_run_params_for_source(__int64 src) {
    if (!src || !CyberpunkVR_RunViewParams) return;
    const uintptr_t params = g_view_params.load(std::memory_order_acquire);
    if (!params) return;
    if (CyberpunkVR_RunViewParams >= 2) {
        vd_apply_params_no_erase(params, src);
    } else {
        if (!g_orig_view_params) return;
        __try {
            g_orig_view_params(params, src);
        } __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    }
    InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugViewParamsRuns));
}

// The copy that carries a freshly produced parameter struct into a view's block. Our own preparer's
// scope is the only thing that opens this, so no other view can be touched by construction.
using ViewDataCopyFn = __int64 (__fastcall*)(__int64, __int64);
static ViewDataCopyFn g_orig_vd_copy = nullptr;

static __int64 __fastcall Detour_ViewDataCopy(__int64 dst, __int64 src) {
    if (src && t_rtt_prepare_ours) vd_run_params_for_source(src);
    return g_orig_vd_copy(dst, src);
}
CVR_DETOUR("[viewparams] viewData copy sub_140294C94", VIEWDATA_COPY_RVA,
           Detour_ViewDataCopy, g_orig_vd_copy)

// One line, so "it did not help" and "it never ran" cannot be confused: every conclusion drawn from
// a silent probe in this project has been wrong.
void view_params_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (!CyberpunkVR_RunViewParams || (s_last && now - s_last < 5000)) return;
    s_last = now;
    Log("[viewparams] mode=%u runs=%llu walked=%llu applied=%llu skip(noobj=%llu erase=%llu) "
        "lock busy=%llu "
        "(MAIN's params seen %llu)\n",
        CyberpunkVR_RunViewParams,
        (unsigned long long)CyberpunkVR_DebugViewParamsRuns,
        (unsigned long long)CyberpunkVR_DebugViewParamsWalked,
        (unsigned long long)CyberpunkVR_DebugViewParamsEntries,
        (unsigned long long)CyberpunkVR_DebugViewParamsSkipNoObj,
        (unsigned long long)CyberpunkVR_DebugViewParamsSkipErase,
        (unsigned long long)CyberpunkVR_DebugViewParamsBusy,
        (unsigned long long)CyberpunkVR_DebugViewParamsSeen);
}

// --- filling the holes ---------------------------------------------------------------------
// The ranges the diff above reports as MAIN-set / VRCAM-zero, measured live. Copying MAIN's
// bytes into them cannot destroy per-view data by construction: VRCAM has nothing there.
// One bit each in CyberpunkVR_ViewDataFixMask so every one can be A/B'd live -- these are
// engine internals we have only partially identified, and a field that looks inert may be a
// count whose array VRCAM does not own.
// Default: the wetness only. It is the one hole whose consumer is identified from the engine's
// own code, so it is the only one that can be turned on without guessing.
// bits 11 and 12 stay OFF. 0x4A0 held MAIN's 400000.0 against VRCAM's zero and looked exactly
// like a far distance -- but of the 68 callers of the viewData getter sub_1401ED930, NOT ONE
// reads +0x4A0. Nothing consumes the field, so filling it is a no-op, which is what the live
// test showed. Kept in the table as a named negative so it is not rediscovered.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_ViewDataFixMask = (1u << 0) | (1u << 5) | (1u << 7);
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewDataFixes = 0;

// A hole we have not identified may hold a pointer, and MAIN's pointer in VRCAM's slot is a
// crash waiting for a dereference. Refuse anything that looks like a user-space address.
// One qword, not a range. The env mirror below copies eight bytes at a time so that a range which
// merely STRADDLES a pointer still delivers its floats instead of being refused whole -- degrade,
// never refuse, which is the rule for anything on the path that makes the VR picture.
static inline bool viewdata_qword_is_pointer(uint64_t q) {
    return q >= 0x10000000000ull && q < 0x7FFFFFFFFFFFull;
}

static bool viewdata_looks_like_pointer(const uint8_t* p, size_t len) {
    if (len < 8) return false;
    for (size_t i = 0; i + 8 <= len; i += 8) {
        uint64_t q;
        memcpy(&q, p + i, 8);
        if (q >= 0x10000000000ull && q < 0x7FFFFFFFFFFFull) return true;
    }
    return false;
}

// Runs on every VRCAM node dispatch, so it stages only the enabled ranges (max 24 bytes each)
// instead of the whole 4 KB block -- cheap enough to be unconditional, which is what makes the
// "before any consumer" guarantee hold without knowing who the consumers are.
// ---- the distant-fog switch the two views disagree on --------------------------------------
//
// Measured, both views, same frame, same spot:
//     0x8C0 {M 1.5 | V 3}   0x8C8 {M 0.75 | V 0.5}   0x8CC {M 1 | V 4}
//     0x918 {M 0.5 | V 1}   0x920 {M 0    | V 0.00025}
//
// 0x920 is not a parameter, it is THE SWITCH. From sub_14061F9E0 (RenderFogOverlay):
//     if ((f0 & 0x800000) == 0 && !sub_1401E4B60(ctx+20)
//         || (v6 = 1, *(float*)(vd + 0x920) <= 0.0))   v6 = 0;
//     if ((f0 & 0x1000000) || (f0 & 0x4000000) || v6) { ... if (v6) <the distant-fog branch> }
// v6 can only survive as 1 when vd+0x920 is strictly positive. MAIN holds exactly 0 there and so
// never enters that branch; VRCAM holds 0.00025 and always does. The second eye runs a
// distant-fog path the first eye does not have, with the parameters beside it 2x to 4x MAIN's.
// A fog the other eye lacks, at four times the strength, buries the far city and washes the
// stars out of the sky at once -- both reports, one field.
//
// Deliberately NOT part of the hole table. Those are safe because VRCAM has nothing in them;
// here it has real values, so copying MAIN's overwrites live per-view data. Start with the
// Start with the switch alone if you must, but all three by default: every measured field
// differed in the same direction, so leaving the parameters at VRCAM's 2x-4x only moves the
// problem to whichever fog path it does take. The risk is real and stated -- unlike a hole
// these overwrite live per-view values, and if one is resolution-derived (0x8CC reads
// M 1 | V 4, which has that shape) the second eye will look wrong in a new way.
//   bit 0   0x920            the switch
//   bit 1   0x918, 0x91C     the scale pair
//   bit 2   0x8C0 .. 0x8D7   the parameter block
// Live: 7 = all, 1 = switch only, 0 = untouched.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_FogMirrorMask = 7;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFogMirrors = 0;
// WIDENED after the first attempt left the night still wrong. Six floats was not the block:
// MAIN's own branch in sub_14061F9E0 does `sub_140201A58(v19 + 35, ...)` with
// v19 = (float*)(viewData + 0x8C0), so the parameter array is 36 floats -- 0x8C0 .. 0x94F --
// and 0x910/0x914 are read by the other branch beside 0x918/0x91C/0x920. Bit 2 now covers all
// of it, which makes bits 0 and 1 subsets kept only for bisecting.
struct FogRange { uint16_t off, len; };
static const FogRange kFogMirror[3] = { { 0x920, 4 }, { 0x910, 0x14 }, { 0x8C0, 0x90 } };

// ---- the atmosphere block, mirrored in bisectable pieces -----------------------------------
//
// Four captures, two at night and two through dawn, printing BOTH views. They split the night
// difference in two, and only one half is weather.
//
// Constant across all four -- configuration, not time:
//     3F0 {M 1.365|V 4.55}   480 {M 1|V 0.75}    4D4 {M 0.03|V 0.06}
//     570 {M 0.7  |V 0.4 }   5A4 {M 1|V 0.766}   5C0 {M 1.3 |V 0.3 }
//     610 {M 8    |V 4   }   61C {M 0.05|V 0.03} 628 {M 0.65|V 1   }   6FC {M 1.4|V 1}
// 8 against 4 and 0.03 against 0.06 have the shape of a march step count and its step size: the
// engine hands the RTT view a cheaper atmosphere. That is the tint, the over-visible mountain
// silhouettes, and why daylight looks the same -- at noon those coefficients barely register.
//
// Moving, but not together -- the weather blend:
//     430 sky radiance  M 8.17 -> 8.62 -> 8.89 -> 131.0
//                       V 2.09 -> 2.09 -> 64.6 -> 68.1
//     3C0 colour        M 0.157 -> 0.155 -> 0.153 -> 0.051
//                       V 0.095 -> 0.095 -> 0     -> 0
// The second view is not frozen; it takes the transition early and on different values, and 3C0
// collapses to zero by dawn. MAIN's smooth 8 -> 131 arc against VRCAM's 2 -> 65 step is exactly
// the reported "there is no gradual lightening".
//
// So mirror MAIN's atmosphere block. This is a BROAD overwrite of live per-view values and it is
// not a root fix -- the root is whatever hands the RTT view its own configuration, which is not
// found yet. Split into bits so a bad range can be bisected out without a rebuild, and with two
// exclusions that are not negotiable: 0x6E0 is a pointer (21B04B08 | 212ACC20) and 0x120..0x143
// is the camera position, which MUST differ between eyes.
//   bit 0  0x370..0x3FF   sun direction and colours
//   bit 1  0x400..0x45F   radiance
//   bit 2  0x460..0x4FF   the 0x480 / 0x4C0 / 0x4D4 group
//   bit 3  0x500..0x5CF   the 0x554 / 0x570 / 0x598 / 0x5A4 / 0x5C0 group
//   bit 4  0x610..0x633 and 0x6FC   the march-step group
// Bits 12-15 are the environment look the second view was never given: the colour grade, the
// brightness/exposure group, the bloom/flare table and the chromatic aberration. All four were read
// out of the two views in the SAME braindance with both views identified the only way that holds --
// the virtual-camera name hash at work_context+0x18 -> +0x28, 0 for MAIN and the port's own key for
// VRCAM -- taken on the port's cloud-CB detour where viewData is a4 - 0x430. The port was running
// with mask 0xFFF at the time, so what was left differing is exactly what nothing else mirrors.
//
// Every field that differed is included, packed words and integer-shaped fields as well, because the
// loop copies a qword at a time and refuses any qword that looks like an address on either side. A
// bad guess here can therefore look wrong; it cannot take the process down.
// DEFAULT IS 0xFFFF, NOT 0x3FFFFF, AND THAT IS A MEASURED RETREAT. Shipping all of bits 12-21 on
// crashed the game before the main menu, twice, at 17 and 19 seconds, in a function family the
// report queue had never shown before this build (exe+0x144C74 and exe+0x144E51, both with R14 =
// FFFFFFFC -- a container index that had gone negative). Bits 16-21 are therefore OUT of the
// default and stay switchable one at a time:
//
//     16  scene bounds        +0120
//     17  the zeroed block    +01A0..+01E7   contains +01A0 = 0x32B6, an integer
//     18  packed words        +0330 +0738
//     19  cloud wind          +05E8
//     20  tagged union        +0FB8          MAIN stores ASCII 'game' where the view stores floats
//     21  small integer       +0FE8          M 0x1461 against V 0x461
//
// The guard makes each of them incapable of carrying an address, which is what it was for; it cannot
// make an integer meaningful in a structure that counts with it. That is the distinction this
// retreat is drawing, and it is the reason these are separate bits rather than one.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_EnvMirrorMask = 0xFFFF;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugEnvMirrors = 0;
// Every eight bytes the guard refused because one side held an address. Not an error: it is how
// a range is allowed to span a pointer at all. A number that climbs with the frame rate means a
// range is mostly pointers and was chosen badly.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugEnvMirrorPtrSkips = 0;
// EXTENDED once the mirror shortened the list from 68 runs to 43 and the printer stopped
// truncating. Two things showed up. Three fields sat just past a boundary -- 0x5D0, 0x634,
// 0x700 -- and a whole second block lives beyond 0x700 that had never been visible:
//     830 {M 125 |V 250}   870 {M 0.3 |V 0.06}   8B0 {M -1  |V 0.015}
//     998 {M 10  |V 16 }   A14 {M 0.5 |V 0.25}   AC4 {M 0.4 |V 0.224}
//     AEC {M 1   |V 0  }   B20 {M 0.05|V 0.15}   B30 {M 0   |V 1}
// Same signature as the first block: constant in time, VRCAM systematically cheaper or stronger.
// That is the residue the user still sees -- a lighter distant background at night and a faint
// white haze in the distance by day.
//   bit 5  0x830..0x8B3     bit 6  0x950..0x967 and 0x998     bit 7  0xA14 and 0xAC4..0xAEF
//   bit 8  0xB20..0xB33
// NOT mirrored, deliberately: 0xBA0/0xBC0/0xBE0/0xBFC/0xC40 differ only in the last digits and
// drift together frame to frame (405B9BA0 against 405B9BB9) -- that is per-eye camera-derived
// data and copying it would break the stereo. 0x6E0, 0xF58, 0xF88, 0xFB8 are pointers/resources.
static const FogRange kEnvMirror[] = {
    { 0x370, 0x90 }, { 0x400, 0x60 }, { 0x460, 0xA0 }, { 0x500, 0xE0 },
    { 0x610, 0x28 }, { 0x6FC, 0x10 },
    { 0x830, 0x88 }, { 0x950, 0x18 }, { 0x998, 0x08 },
    { 0xA14, 0x04 }, { 0xAC4, 0x34 }, { 0xB20, 0x18 },
    // bit 9: the packed word 0xA18 {M 00000601 | V 00000001}.
    // bit 10: viewData carries the atmosphere block TWICE. 0xBD0 {0.157|0.0951} and
    // 0xBE0 {1.365|4.55} are the same two pairs already seen at 0x3C0 and 0x3F0 -- mirroring
    // the first copy left the second untouched, which is why a residue survived. The camera
    // -derived neighbours 0xBA0/0xBC0/0xBFC/0xC40 stay excluded.
    { 0xA18, 0x04 }, { 0xBD0, 0x20 },
    // bit 12: THE COLOUR-GRADE SECTION, and it is the whole environment look, not a detail.
    //
    // Traced in the debugger 2026-08-30, from the shader backwards. The 688-byte grading constant
    // block is filled by sub_1403AB104(shared, viewData, deltas, out), and every value it writes
    // it reads out of THIS view's viewData:
    //
    //     movss xmm0,[rdx+0x650]   addss xmm0,[r8+0x00]   movss [r9+0x70],xmm0
    //     ... +0x654 +0x658 +0x65C +0x660 +0x664 +0x668, then movups at +0x670 +0x680 ... +0x6D0
    //
    // The r8 addends were measured as zero, so the output is that region verbatim. Scanning the
    // function's own code for every [rdx+disp32] it touches gives the exact extent: 0x640..0x6D0,
    // i.e. 160 bytes -- which this table brackets on both sides ({0x610,0x28} and {0x6FC,0x10})
    // and skips.
    //
    // In a braindance MAIN's copy holds 24h_braindance_fpp.envparam -- gain 1.3 on green and blue,
    // saturation 1.3, contrastPivot 0.435, gamma 0.96 -- and the second view's holds the identity.
    // That is the green cast, and the same region carries the rest of the envparam's look, so the
    // bloom, chromatic aberration and fog arrive with it rather than needing a mirror each.
    //
    // RANGE STARTS AT 0x650, NOT 0x640, and that is not a detail: +0x644 holds a POINTER
    // (read live: 0x1DAB46DEA38). The producer touches +0x640 as well, but copying MAIN's
    // pointer into the second view's slot is the dereference-waiting-to-happen this file warns
    // about everywhere else. From 0x650 on it is floats only, verified by reading the region
    // out of both views.
    //
    // CONFIRMED ON THE PICTURE, 2026-08-30: with this bit on, dropping the grading-block mirror
    // from 0x7F03 to 3 left the braindance's green cast on the second eye. The tint travels this
    // way, so the constant-block substitution in Grading.cpp is retired to the descriptor pair.
    { 0x650, 0x80 },
    // bit 13: THE BRIGHTNESS/EXPOSURE GROUP, 0x788..0x7A7. +0x798 is the field a whole evening of
    // live bisection already pinned as the second eye's darkness (see the note in this project's
    // history: MAIN 0.0 against the view's 3.0), and it has never been mirrored -- only claimed and
    // withdrawn. It sits in a run of five that differ together, so the run travels together:
    //
    //     +078C   M 1          V 0.5
    //     +0790   M 1          V 2        integer-shaped, kept anyway: the guard below cannot crash
    //     +0798   M 0          V 3        the darkness
    //     +079C   M 0.673077   V 0.8      the exposure multiplier
    //     +07A0   M 0.445513   V 0.25
    //
    // CONFIRMED ON THE PICTURE, 2026-08-30, AND IT IS THE MISSING GLOW. At mask 2FFF -- this bit
    // alone of the four, with the grade bit off -- the second eye lost the green cast and KEPT the
    // bloom. So the braindance's blown-out look comes from this group, not from the bloom
    // parameters, which is the same conclusion the identical +0x0A20..+0x0A70 block already forced.
    { 0x788, 0x20 },
    // bit 14: THE BLOOM/FLARE TABLE, in two runs. Measured in the same braindance frame:
    //
    //     +07B8   M 000000FF   V 912FE3FF   packed word
    //     +07BC   M 100        V 1000
    //     +07C0   M 100        V 700
    //     +07E0   M 1          V 1.59645
    //     +07E8   M 1          V 1.2
    //
    // This is the strongest remaining candidate for the missing glow, and it is a SEPARATE bit from
    // 13 for exactly that reason: the bloom block proper at +0x0A20..+0x0A70, the one holding
    // numDownsamplePasses = 6, is byte-for-byte IDENTICAL between the two views, so the bloom
    // parameters are not what differ. If the second eye gains its glow with 14 on and 13 off, it is
    // this table; if with 13 and not 14, it is the exposure. One session separates them.
    //
    // NOT YET SEEN ON THE PICTURE: the glow turned out to ride on bit 13, so this group has no
    // demonstrated effect of its own. It stays on because the values are measured and wrong in the
    // second view (100 against 1000 and 700), not because anything has been attributed to it.
    { 0x7B8, 0x10 }, { 0x7E0, 0x10 },
    // bit 15: CHROMATIC ABERRATION. 24h_braindance_fpp.envparam sets chromaticAberrationSize 2.7,
    // 2.7 and MAIN carries exactly that; the second view carries 0.8 / 0.3.
    //
    //     +0A04   M 0          V 0.019277
    //     +0A08   M 2.7        V 0.8
    //     +0A0C   M 2.7        V 0.3
    { 0xA00, 0x10 },
    // bit 16: THE SCENE BOUNDS. MAIN carries a real world AABB, the second view carries +-FLT_MAX,
    // i.e. it was never given one:
    //
    //     +0120   M -21246  -49610.8  -51018.1  1     V  7F7FFFFF x4
    //     +0130   M  11842.6 50389.2   49271.9  1     V  FF7FFFFF x4
    //
    // These are WORLD-space bounds, the same for both eyes by construction, so mirroring cannot
    // collapse the stereo. Its own bit because unbounded-to-bounded is the direction that can start
    // clipping something.
    { 0x120, 0x20 },
    // bit 17: THE BLOCK THE SECOND VIEW HOLDS AT ZERO. MAIN has worked values, the view has 0 or a
    // round default -- the shape this file has already fixed three times:
    //
    //     +01A0   M 000032B6   V 0
    //     +01AC   M 409.367    V 771.843
    //     +01B4   M 146.414    V 0
    //     +01B8   M 0.026764   V 0.0333333     a frame delta: 1/37.4 against a flat 1/30
    //     +01BC   M 0.026764   V 0
    //     +01CC   M 1.00008    V 0
    //     +01D0   M 9289.85    V 0
    //     +01D4   M 9289.82    V 0
    //     +01DC   M -1.37623   V 0
    //     +01E0   M 9.09568    V 0
    //
    // A view whose clock and counters read zero is the documented reason nothing animated moves in
    // the second eye, so this group is worth more than its size suggests.
    { 0x1A0, 0x48 },
    // bit 18: THE TWO PACKED WORDS, 0x330 (00000100 against BF4DFB9E) and 0x738 (8F010101 against
    // F5010101). Excluded by hand in an earlier round for being packed rather than float; the
    // qword guard above makes that caution unnecessary, so they are in.
    { 0x330, 0x08 }, { 0x738, 0x08 },
    // bit 19: THE CLOUD WIND, +05EC..+0600, six floats MAIN has and the view has at zero. There is
    // an older hole bit for the same offsets that routes through the cloud constant buffer; this one
    // reaches viewData itself, which is what the per-view producers read.
    { 0x5E8, 0x20 },
    // bit 20: +0FB8..+0FCF. MAIN's +0FB8 reads 656D6167 ('game' in ASCII) where the view holds
    // floats -- a tagged union, so the two sides are not even the same shape. Kept because the guard
    // makes it safe to find out, and separated so it can be dropped alone if it looks wrong.
    { 0xFB8, 0x18 },
    // bit 21: +0FE8, a small integer, M 00001461 against V 00000461.
    { 0xFE8, 0x08 },
    //
    // WHAT IS DELIBERATELY LEFT OUT, and NOT because it would crash -- the guard handles crashes:
    //
    //   +0268/+026C  THE SECOND VIEW'S OWN CAMERA-NAME HASH. MAIN reads 0x400 there, the view reads
    //                its key 2A88938DEDF136E6. Copying MAIN's over it does not fault -- it makes the
    //                port unable to tell the two views apart, and every hash-identified fix in this
    //                codebase silently falls back to guessing. Never enable ViewDataFixMask bit 4
    //                either; its entry {0x268,4} is labelled 'composition out' and that label is
    //                wrong.
    //   the pointer fields at +0030..+0058, +0140, +0168..+0174, +06E0, +0F58, +0F88, +0FE0, +0FF0
    //                are per-view RESOURCES (composition output, resource sets). The guard would
    //                refuse them anyway, so listing them would only burn cycles.
    // bit 11: SIX FIELDS WHERE THE SECOND VIEW IS HOLDING FACTORY DEFAULTS. Read out of the live
    // [vdiff] viewData channel, MAIN against VRCAM, in the interior where the mismatch shows:
    //
    //     0x720   M 1.0000   V 4000.0
    //     0x778   M 1.0669   V 1.2
    //     0x7E4   M 0.3297   V 0.19
    //     0xA10   M 6.0023   V 7.0
    //     0xA1C   M 0.2567   V 0.24
    //     0xF20   M 0.0000   V 0.15
    //
    // The shape is what makes this worth a test rather than a shrug: every VRCAM value is a ROUND
    // number and every MAIN value is a worked one. That is not two views disagreeing about a
    // camera-derived quantity, it is one view whose fields were filled from the environment and one
    // whose were left at whatever the constructor wrote -- the same failure this file already fixed
    // twice, at 0x920 for the distant fog and across the atmosphere block above.
    //
    // WHY THESE SIX AND NOT THE OTHER FIVE that also differ. 0x330, 0x738, 0x7A4 and 0x7F0 are
    // packed byte words (FEFEFE08 against FFFFFF08) and 0x6E8 is a small integer (2 against 1) that
    // has the shape of a per-view quality level; copying either kind blind is how a mirror breaks
    // something new. These six are plain floats.
    //
    // RESULT: THE MIRROR WORKS AND THE SYMPTOM DOES NOT FOLLOW IT. Confirmed from the live [vdiff]
    // channel rather than assumed -- all six offsets dropped out of the differing list and the run
    // count fell from 36 to 24-31 -- so this is a real negative result and not an inert knob, which is
    // the distinction that cost a round earlier in the same hunt (CyberpunkVR_ProbeReuseMode).
    // The ceiling is still about 1.8x brighter in VRCAM, so whatever does that is elsewhere.
    //
    // KEPT ON anyway, at the user's call and for a good reason: the six were genuinely wrong, holding
    // constructor defaults in a view whose neighbours were all filled from the environment. Leaving a
    // known-wrong value in place because it does not happen to cause the bug being chased is how the
    // next bug gets an extra variable. Nothing regressed with it on.
    //
    // FAILURE MODE, if a later change makes one of them matter: a legitimately per-view field would
    // make the second eye wrong at a DIFFERENT distance or brightness rather than matching. Clear bit
    // 11 and bisect by splitting the group.
    //
    // AND 0x720 TURNS OUT TO FEED SCREEN-SPACE REFLECTIONS, which is where the interior defect actually
    // lives. The chain, measured end to end on frame_10255 (captured 11:51, i.e. BEFORE this bit shipped
    // at 12:03, so it still shows the unmirrored state):
    //
    //   1. a grid over the shared scene HDR target, read at the same stage in each view, peaks at 1.84x
    //      on the ceiling and sits at ~1.00 over two thirds of the frame
    //   2. pixel history on one ceiling pixel says the difference is introduced by RenderLightsIntegrate,
    //      not by the fog: VRCAM 0.0879/0.0347/0.0278 against MAIN 0.0322/0.0034/0.0059 after lighting,
    //      and the fog then adds +0.049 to BOTH
    //   3. of the sixteen textures that action reads, fifteen agree at that pixel and ONE does not:
    //      ResourceId::32983, R8_UNORM 2560x2560, VRCAM 1.0000 against MAIN 0.0157
    //   4. that mask is written by ScreenSpaceReflections (RVA 0x157B24), three dispatches in EACH view --
    //      so the second view does run the pass; it just gets a different answer
    //   5. its 372-byte constant block differs in two fields: [22].y = 4000 against 1, and
    //      [22].z = 5000 against 8. The 4000/1 pair is exactly viewData+0x720, which bit 11 now mirrors.
    //
    // A ray march told to go 4000 units in 5000 steps finds a hit almost everywhere, which is why the
    // second view reflects the room off a ceiling that MAIN leaves matt. It also explains why the fog,
    // GI, probe and local-shadow knobs each changed nothing: none of them is in this path.
    //
    // STILL OPEN: where [22].z (5000 against 8) comes from. It matches none of the viewData fields this
    // file mirrors, and the 372-byte block never appears in the [wide] census, so it is not uploaded
    // through either constant path this port hooks -- it is suballocated out of the long-lived buffer
    // ResourceId::290. Do not hook it before that source is identified.
    //
    // The measurement to compare against, from frame_10255: at the SAME stage of the same shared
    // target (right after each view's RenderFogOverlay), a grid of ratios is ~1.00 across two thirds
    // of the frame and peaks at 1.84x over x 1280..1600 y 640..960 -- the ceiling. Everything feeding
    // that region has been measured equal: the lighting integrate (9 identical bindings, same target),
    // the volumetric fog, the 160x160 atmosphere LUTs (0.975-0.979 per band), the exposure block, and
    // every fog parameter in viewData (the [fog] probe prints 0x920 as 0.00025 in BOTH views indoors,
    // which also retires the older note here claiming MAIN holds exactly 0 there).
    // 0x720 IS A PAIR, AND THE SECOND HALF IS THE HALF THAT MATTERS. Mirroring four bytes here
    // equalised 0x720 and the very next log line showed a fresh run starting at 0x724 with
    // {41000000|459C4000} -- 8.0 against 5000.0. Those two dwords are the screen-space-reflection
    // march: distance and step count. IDA confirms they are one pair rather than two fields, twice
    // over: sub_14024DE10 writes 4000.0 to [r10+0x310] and 5000.0 to [r10+0x314], and sub_1406109CC
    // writes the same two to [r8+0x48] and [r8+0x4C] -- adjacent floats, default-initialised.
    //
    // A ray march told to travel 4000 units in 5000 steps finds a hit almost everywhere, which is
    // why the second view mirrors the room in a ceiling MAIN leaves matt. Measured end to end on
    // frame_10255: the SSR mask ResourceId::32983 reads 1.0000 in VRCAM against 0.0157 in MAIN at
    // the same ceiling pixel, and RenderLightsIntegrate multiplies by it.
    // AND THE SAME TAIL MISTAKE APPLIED TO THE OTHER FIELDS, now visible because the diff prints run
    // lengths. After the four-byte mirrors landed, the log showed fresh runs immediately after them:
    //
    //     77C*1{3F08C427|3F19999A}   MAIN 0.5342   VRCAM 0.6     -- tail of the run that starts 0x778
    //     A20*2{40F57A98|41000000}   MAIN 7.6708   VRCAM 8.0     -- a pair just past 0xA10..0xA1F
    //
    // Same signature that has now paid off twice: a round number in the second view against a worked
    // one in MAIN. 0x778 therefore covers eight bytes, and 0xA20 is added as its own pair. If the log
    // then shows a run at 0x780 or 0xA28, these ranges are still short -- the length field says so
    // directly now, so size them from the log rather than extending them by guess.
    // CONFIRMED FIXED, by the same measurement that found it (tools/rdc_viewgrid.py, node
    // RenderFogOverlay, frame_10040 against frame_10255):
    //
    //     before   ceiling cells 1.74 1.84 1.65 1.43, worst 1.84x at y 640..960
    //     after    ceiling cells 0.98 .. 1.06, median across the frame 0.995
    //
    // The six cells still off by more than 15% are all in the BOTTOM row (y 2240..2560) and were
    // off there before the fix too (0.54 .. 1.11), which is parallax on the nearest geometry --
    // floor, hands, weapon -- not a per-view mismatch. 64 mm of eye separation moves a half-metre
    // object far more than a three-metre ceiling. The user could no longer tell the two eyes apart
    // by eye, which is what the numbers say too.
    { 0x720, 0x08 }, { 0x778, 0x08 }, { 0x7E4, 0x04 },
    { 0xA10, 0x04 }, { 0xA1C, 0x04 }, { 0xA20, 0x08 }, { 0xF20, 0x04 },
};
static const uint32_t kEnvBit[] = { 0, 1, 2, 3, 4, 4, 5, 6, 6, 7, 7, 8, 9, 10,
                                    12, 13, 14, 14, 15,
                                    16, 17, 18, 18, 19, 20, 21,
                                    11, 11, 11, 11, 11, 11, 11 };
// Counted, never hard-coded. The loop below used to say `k < 14` beside a table of 14, and this
// project has already lost days to fixed-size tables that silently stopped covering their contents.
static const uint32_t kEnvCount = sizeof(kEnvMirror) / sizeof(kEnvMirror[0]);
static_assert(sizeof(kEnvBit) / sizeof(kEnvBit[0]) == sizeof(kEnvMirror) / sizeof(kEnvMirror[0]),
              "kEnvBit and kEnvMirror must stay the same length");


static void viewdata_fill_holes(__int64 vd) {
    const uint32_t mask = CyberpunkVR_ViewDataFixMask;
    const uint32_t fogm = CyberpunkVR_FogMirrorMask;
    if (!vd || (!mask && !fogm && !CyberpunkVR_EnvMirrorMask)
        || !g_vd_have.load(std::memory_order_acquire)) return;
    uint8_t staging[64];
    for (size_t i = 0; i < sizeof(kViewDataHoles) / sizeof(kViewDataHoles[0]); ++i) {
        if (!(mask & (1u << i))) continue;
        const ViewDataHole& h = kViewDataHoles[i];
        if (h.len > sizeof(staging)) continue;
        {
            std::lock_guard<std::mutex> lk(g_cloud_cb_mtx);
            memcpy(staging, g_vd_main + h.off, h.len);
        }
        if (viewdata_looks_like_pointer(staging, h.len)) continue;
        if (cloud_cb_raw_copy(reinterpret_cast<uint8_t*>(vd) + h.off, staging, h.len))
            InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugViewDataFixes));
    }
    for (int k = 0; k < 3 && fogm; ++k) {
        if (!(fogm & (1u << k))) continue;
        const FogRange& f = kFogMirror[k];
        uint8_t fstage[0x90];              // the block is 144 bytes; `staging` is only 64
        if (f.len > sizeof(fstage)) continue;
        {
            std::lock_guard<std::mutex> lk(g_cloud_cb_mtx);
            memcpy(fstage, g_vd_main + f.off, f.len);
        }
        if (cloud_cb_raw_copy(reinterpret_cast<uint8_t*>(vd) + f.off, fstage, f.len))
            InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugFogMirrors));
    }
    const uint32_t envm = CyberpunkVR_EnvMirrorMask;
    for (uint32_t k = 0; k < kEnvCount && envm; ++k) {
        if (!(envm & (1u << kEnvBit[k]))) continue;
        const FogRange& f = kEnvMirror[k];
        uint8_t estage[0x100];
        uint8_t dstage[0x100];
        if (f.len > sizeof(estage)) continue;
        {
            std::lock_guard<std::mutex> lk(g_cloud_cb_mtx);
            memcpy(estage, g_vd_main + f.off, f.len);
        }
        uint8_t* const dst = reinterpret_cast<uint8_t*>(vd) + f.off;
        // A LENGTH THAT IS NOT A WHOLE NUMBER OF QWORDS KEEPS THE OLD WHOLE-RANGE SEMANTICS. The loop
        // below cannot express a 4-byte tail, and an earlier version of this code simply skipped such
        // ranges -- which silently switched off SEVEN entries that had been working, four of them in
        // bit 11: +07E4, +0A10, +0A14, +0A18, +0A1C, +0AC4, +0F20. It was caught only because the two
        // views were diffed again afterwards and offsets that had agreed for weeks came back apart.
        // This project has already lost days to tables that quietly stop covering their contents; do
        // not reintroduce a shape where an entry can be ignored without saying so.
        if (f.len < 8 || (f.len & 7)) {
            if (viewdata_looks_like_pointer(estage, f.len)) continue;
            if (cloud_cb_raw_copy(dst, estage, f.len))
                InterlockedIncrement64(
                    reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugEnvMirrors));
            continue;
        }
        // Read what the second view currently holds, so a POINTER can be refused from either side:
        // MAIN's pointer written into this view is a dereference waiting to happen, and MAIN's float
        // written over a pointer this view still owns is the same crash from the other direction.
        // The whole reason the ranges below can be generous -- packed words, unidentified fields and
        // all -- is that this loop can never carry an address across.
        if (!cloud_cb_raw_copy(dstage, dst, f.len)) continue;
        bool any = false;
        for (uint32_t off = 0; off + 8 <= f.len; off += 8) {
            uint64_t sq, dq;
            memcpy(&sq, estage + off, 8);
            memcpy(&dq, dstage + off, 8);
            if (viewdata_qword_is_pointer(sq) || viewdata_qword_is_pointer(dq)) {
                InterlockedIncrement64(
                    reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugEnvMirrorPtrSkips));
                continue;
            }
            if (sq == dq) continue;                       // already agrees; do not touch it
            if (cloud_cb_raw_copy(dst + off, estage + off, 8)) any = true;
        }
        if (any)
            InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugEnvMirrors));
    }
}

// Defined further down, beside the composition-state detour that learns the set pointer.
static void vd_lend_composition(uintptr_t vd);

// The engine's own accessor is (*wc)->vt[4](); inlined here so the fill can run from the node
// dispatch, which does not otherwise have the view object in hand.
void viewdata_fill_from_wc(void* wc) {
    uintptr_t vd = 0;
    __try {
        const uintptr_t obj = wc ? *reinterpret_cast<uintptr_t*>(wc) : 0;
        const uintptr_t vt  = obj ? *reinterpret_cast<uintptr_t*>(obj) : 0;
        if (vt) {
            using ViewDataFn = uintptr_t(__fastcall*)(uintptr_t);
            const ViewDataFn fn = *reinterpret_cast<ViewDataFn*>(vt + 32);
            if (fn) vd = fn(obj);
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    // Before the holes, and before this dispatch's work function runs: the composition gate has
    // to be in the block by the time the FIRST of our nodes runs, not merely by the time the
    // composition node does -- see vd_lend_composition.
    vd_lend_composition(vd);
    if (vd) viewdata_fill_holes(static_cast<__int64>(vd));
}

static __int64 __fastcall Detour_CloudCbFill(__int64 a1, __int64 a2, __int64 a3, __int64 a4,
                                             __int64 a5, int* a6, int a7, int a8) {
    const __int64 r = g_orig_cloud_cb(a1, a2, a3, a4, a5, a6, a7, a8);
    const int v = (CyberpunkVR_CloudCbMode || CyberpunkVR_ViewDataDiff)
                      ? cloud_cb_view(a3) : -1;
    if (CyberpunkVR_CloudCbMode) {
        if (v == 0)      cloud_cb_capture_main(a2);    // a2 is the constant buffer being filled
        else if (v == 1) { if (CyberpunkVR_ViewDataDiff >= 2) cloud_cb_diff_vrcam(a2);
                           cloud_cb_apply_vrcam(a2); }
    }
    if (a4 && (CyberpunkVR_ViewDataDiff || CyberpunkVR_ViewDataFixMask)) {
        const __int64 viewData = a4 - 0x430;
        __int64 ctx = 0;
        if (CyberpunkVR_ViewDataDiff) {
            __try { ctx = *reinterpret_cast<__int64*>(a3 + 0x18); }
            __except (EXCEPTION_EXECUTE_HANDLER) { ctx = 0; }
        }
        if (v == 0) {
            viewdata_capture_main(viewData);
            if (ctx) ctx_capture_main(ctx);
        } else if (v == 1 && CyberpunkVR_ViewDataDiff) {
            if (ctx) ctx_diff_vrcam(ctx);
            // The fill itself now happens at node dispatch (far earlier); by the time this runs
            // the enabled holes are already closed, so they drop out of the report.
            viewdata_diff_vrcam(viewData);
        }
    }
    return r;
}

// --- Local-shadow VSM reuse for VRCAM ------------------------------------
// CRenderNode_RenderLocalShadowMaps sub_140AD5770 renders per-light local shadow maps
// into the SHARED committed VSM atlas (Resource_26424/26425, 512x512x10 R16G16) indexed
// by a SHARED slot table: mgr = *(ctx+0x1E10), slots @ mgr+801280, count @ mgr+801292.
// VERIFIED live IDENTICAL for vrcam & main (mgr 0x179157472C0 / slots 0x175279EAD20)
// => same light->slice mapping, so reuse is index-correct. Local shadows are world-space
// per-light + temporally cached; VRCAM (fresh view) forces a full re-render (10.6x main's
// cycles) into the shared atlas -> thrashes MAIN's cache -> both re-render every frame ->
// shadow flicker + perf. Reuse = skip vrcam's node so it neither re-renders nor advances
// the shared manager; vrcam lighting samples MAIN's atlas via the shared manager. Same ABI
// as distant: a2=rdx, view ctx = *(a2+0x18), key @ ctx+0x28. If vrcam loses local shadows
// after test, the fallback is to skip only the render sub_140153260 (keep node's fg decls).
using LocalShadowFn = __int64(__fastcall*)(void*, void*);
static LocalShadowFn g_orig_local_shadow = nullptr;
// DEFAULT 0 since 2026-07-29. The per-node dispatch census leaves exactly one lighting-
// relevant node that never dispatches for VRCAM, and it is this one, skipped by us. It fits
// the symptom better than anything else measured: some lights work for VRCAM (near stalls)
// and some never do (street lamps, the road) -- which is what a missing shadow slice looks
// like, since a shadow-casting light with no slice is dropped rather than drawn unshadowed.
// Tested before only together with the other two reuse modes, where VRCAM re-rendering every
// local shadow (30x MAIN in the audit) could break it a second way.
// Measured 2026-07-29: with this at 0 the node does dispatch for VRCAM (AD5770 leaves the
// census) and the lamps are still unlit. Not the cause; put back for the 30x it costs.
// AND TESTED AT 0 AGAIN 2026-08-18 AGAINST A SYMPTOM IT HAD NEVER BEEN TRIED ON, WITH THE SAME ANSWER:
// interior lighting differs between the eyes -- the sofa cushions and the ceiling, "shiny in VRCAM,
// normal in MAIN". Outdoors the sun dominates and this is invisible; indoors the local lights ARE the
// lighting, and this is the one lighting-relevant node the second view does not run, so it was the first
// thing to try. At 0 the artefacts are UNCHANGED (user, same scene). Local shadow reuse is not the cause
// of the interior mismatch either, and this knob is back at 1 for the 10.6x it costs.
//
// That also kills the analogy I reached for. A sun cascade is fitted to the VIEWER, which is what gave
// the two views something to disagree about; local shadow maps are per-light and world-space with a
// verified-identical light->slice mapping, so reuse here is exact by construction and there was never a
// matrix for the eyes to disagree over. Next suspect is the specular path itself -- reflection probes and
// screen-space reflections -- not the shadow atlases.
// (previous note kept below)
// AT 0 2026-08-18 for the interior symptom: interior lighting differs
// between the eyes -- the sofa cushions and the ceiling, "shiny in VRCAM, normal in MAIN". Outdoors the
// sun dominates and this is invisible; indoors the local lights ARE the lighting, and this is the one
// lighting-relevant node the second view does not run.
//
// Read the result with the note above in mind. At 0 the second view re-renders every local shadow into
// the shared atlas, which is measured at 10.6x MAIN's cycles and thrashes MAIN's temporal cache, so
// shadow flicker and a frame-time cost at 0 are EXPECTED and are not new bugs. What the test answers is
// only whether the interior specular mismatch follows this knob.
//
// And one analogy is withdrawn before it misleads: this is NOT the sun-cascade case. A cascade is fitted
// to the viewer, which is what gave the two views something to disagree about. Local shadow maps are
// per-light and world-space, and the light->slice mapping was verified live identical, so reuse here is
// exact by construction. If the interior mismatch does follow this knob, the mechanism is something else
// and has to be found rather than assumed.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_LocalShadowReuseMode = 1;   // 1=reuse/skip, 0=vrcam renders its own (A/B)
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLocalShadowSkipHits = 0;

static bool local_shadow_is_vrcam(void* a2) {
    if (CyberpunkVR_LocalShadowReuseMode != 1 || !a2) return false;
    __try {
        uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
            reinterpret_cast<uint8_t*>(a2) + 0x18);
        return ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
// --- THE FOUR RESOURCE POINTERS THE ATMOSPHERE MIRROR LEAVES ALONE -------
//
// Where this lands after three disproven guesses. The sky node's gates were measured and all three pass for
// BOTH views (rect (0,0)..(2560,2560) non-empty, feature bit 35 set, the global set, node entered 6191 times),
// so the second view does compute sky scattering and "MAIN-only" was an artefact of marker-based pass listing.
// Reflection-probe reuse and local-shadow reuse were each armed and each changed nothing.
//
// What no measurement has explained away is the split the capture found directly: the two views run the
// atmosphere/volumetric-fog chain on DIFFERENT resources.
//
//     VRCAM  reads 26609  ->  writes 707881  ->  samples it 81 times
//     MAIN   reads 45412  ->  writes 26608   ->  samples it 78 times
//     45412 is never written in the frame; 26609 is written by a copy at event 47295, i.e. during MAIN's
//     turn, AFTER the second view already read it
//
// And the atmosphere mirror right above says where such a split would live: its own note lists what it refuses
// to copy -- "0x6E0, 0xF58, 0xF88, 0xFB8 are pointers/resources". Those are per-view RESOURCE slots in
// viewData, excluded on purpose because copying a pointer is not copying a float. If one of them is the LUT the
// fog samples, that exclusion is exactly the split.
//
// READ ONLY. Four pointers per view, printed side by side, so the next step is chosen from what is in them
// rather than from a fourth guess. Nothing is copied: a resource pointer mirrored blind is how a view ends up
// sampling a texture that has been freed.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_EnvPtrProbe = 1;

namespace {
// Its own pointer to the viewData getter: the cascade code declares one too, but that lives further down
// the file and a probe should not depend on declaration order.
using EnvViewDataFn = void*(__fastcall*)(void*);
EnvViewDataFn g_env_viewdata = nullptr;

constexpr uint16_t kEnvPtrOff[4] = { 0x6E0, 0xF58, 0xF88, 0xFB8 };
uintptr_t g_envptr[2][4] = {};
bool      g_envptr_have[2] = {};

void envptr_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    char line[600];
    int used = 0;
    line[0] = 0;
    for (int k = 0; k < 4; ++k)
        used += snprintf(line + used, sizeof(line) - used, "+0x%X M=%p V=%p%s  ",
                         kEnvPtrOff[k], (void*)g_envptr[0][k], (void*)g_envptr[1][k],
                         (g_envptr[0][k] == g_envptr[1][k]) ? " same" : " DIFFER");
    log("[envptr] viewData resource slots the atmosphere mirror skips: %s%s", line,
        (g_envptr_have[0] && g_envptr_have[1]) ? "" : " (one side not seen yet)");
}
}  // namespace

// --- THE EMPTY-RECT GATE, read where the engine reads it -----------------
//
// IDA named it, headless, on the existing database (tools/ida_sky_probe.py):
//
//     sub_1407818B0  RenderSkyScattering node work
//         if ( sub_14023AF5C(a2, 35) )                     // feature bit 35
//             if ( !sub_1401E4B60(*(a2+24) + 20) )         // 24 = 0x18 = the view ctx, then +0x14
//                 if ( *(qword_143427C00 + 200) ) sub_1407818F8(...)   // the actual work
//
//     bool sub_1401E4B60(_DWORD *a1) { return *a1 >= a1[2] || a1[1] >= a1[3]; }
//
// That is an EMPTY-RECTANGLE test on four dwords at ctx+0x14: x0>=x1 || y0>=y1. Sky scattering runs only
// while that rect is NON-empty. The same predicate gates the distant-fog branch -- see the fog mirror note
// above, which had already reverse-engineered `!sub_1401E4B60(ctx+20)` inside sub_14061F9E0 -- and it gates
// RenderRainMap too: two of its 67 call sites, 0x3726F2 and 0x3727EB, are inside node 0x3726CC.
//
// So ONE pair of fields decides whether a view gets sky scattering, the rain map and the distant fog. That is
// the shape of the reported symptom: interior lighting wrong in every way at once -- glasses lit blue like the
// sky instead of yellow like the room, no haze in the distance, blown highlights, wrong window reflections --
// rather than five separate bugs. And it agrees with the capture from the other side: RenderSkyScattering and
// RenderRainMap are MAIN-only there, and the second view samples its own atmosphere LUT chain.
//
// This is a PROBE and nothing else: it reads sixteen bytes and prints them per view. Fixing the rect before
// knowing what is in it would be the guess this project keeps paying for.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_ViewRectProbe = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugViewRectReads = 0;

namespace {
// ALL THREE CONDITIONS, not one. The rect was measured first and came back innocent -- (0,0)..(2560,2560)
// and non-empty for BOTH views, with the node running 8747 times -- so the second view reaches this node and
// passes that gate. The remaining two are the feature-bit test and a global, and guessing which is the third
// guess in a row this file would have paid for. So the probe now evaluates exactly what the engine evaluates:
//
//     sub_14023AF5C(a2, 35)             the feature bit, called through the engine's own test
//     !sub_1401E4B60(ctx + 0x14)        the empty-rect test (already innocent)
//     *(qword_143427C00 + 200)          a global, expected view-independent
struct ViewRect { int32_t x0, y0, x1, y1; int bit35, glob; bool have; };
ViewRect g_viewrect[2] = {};      // [0] = MAIN, [1] = second view

using FeatureBitFn = bool(__fastcall*)(void*, int);

void viewrect_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    const ViewRect& m = g_viewrect[0];
    const ViewRect& v = g_viewrect[1];
    log("[viewrect] sky node gates -- MAIN rect (%d,%d)..(%d,%d) empty=%d bit35=%d glob=%d | "
        "VRCAM rect (%d,%d)..(%d,%d) empty=%d bit35=%d glob=%d | reads=%llu%s",
        m.x0, m.y0, m.x1, m.y1, (m.have && (m.x0 >= m.x1 || m.y0 >= m.y1)) ? 1 : 0,
        m.bit35, m.glob,
        v.x0, v.y0, v.x1, v.y1, (v.have && (v.x0 >= v.x1 || v.y0 >= v.y1)) ? 1 : 0,
        v.bit35, v.glob,
        (unsigned long long)CyberpunkVR_DebugViewRectReads,
        (m.have && v.have) ? "" : "  (one side not seen yet)");
}
}  // namespace

using SkyScatterFn = __int64(__fastcall*)(void*, void*);
static SkyScatterFn g_orig_sky_scatter = nullptr;

static __int64 __fastcall Detour_SkyScattering(void* a1, void* a2) {
    if (CyberpunkVR_ViewRectProbe && a2) {
        __try {
            const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
                reinterpret_cast<uint8_t*>(a2) + 0x18);
            if (ctx) {
                const int32_t* r = reinterpret_cast<const int32_t*>(ctx + 0x14);
                const bool vrcam = *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
                ViewRect& slot = g_viewrect[vrcam ? 1 : 0];
                slot.x0 = r[0]; slot.y0 = r[1]; slot.x1 = r[2]; slot.y1 = r[3];
                // The feature bit, asked the way the node asks it. -1 means the test could not be reached.
                slot.bit35 = -1;
                if (g_exe_base) {
                    auto fb = reinterpret_cast<FeatureBitFn>(
                        reinterpret_cast<uint8_t*>(g_exe_base) + FEATURE_BIT_TEST_RVA);
                    slot.bit35 = fb(a2, 35) ? 1 : 0;
                }
                // The global the node checks last: *(qword_143427C00 + 200). View-independent by nature, so
                // if it reads 0 the pass is off for BOTH views and the asymmetry is elsewhere entirely.
                slot.glob = -1;
                if (g_exe_base) {
                    const uintptr_t pp = *reinterpret_cast<uintptr_t*>(
                        reinterpret_cast<uint8_t*>(g_exe_base) + 0x3427C00);
                    slot.glob = (pp && *reinterpret_cast<uintptr_t*>(pp + 200)) ? 1 : 0;
                }
                slot.have = true;
                InterlockedIncrement64(
                    reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugViewRectReads));
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { }
        viewrect_report();
    }
    if (CyberpunkVR_EnvPtrProbe && a2 && g_exe_base) {
        if (!g_env_viewdata)
            g_env_viewdata = reinterpret_cast<EnvViewDataFn>(
                reinterpret_cast<uint8_t*>(g_exe_base) + 0x1ED930);
        __try {
            uint8_t* vd = reinterpret_cast<uint8_t*>(g_env_viewdata(a2));
            const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
                reinterpret_cast<uint8_t*>(a2) + 0x18);
            if (vd && ctx) {
                const int side = (*reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key) ? 1 : 0;
                for (int k = 0; k < 4; ++k)
                    g_envptr[side][k] = *reinterpret_cast<uintptr_t*>(vd + kEnvPtrOff[k]);
                g_envptr_have[side] = true;
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { }
        envptr_report();
    }
    return g_orig_sky_scatter(a1, a2);
}

// --- Reflection-probe atlas reuse for VRCAM -----------------------------
// THE ONE SHARED CACHE THIS PORT NEVER GUARDED. The reuse table had GI, distant shadows, local shadow maps
// and the temporal history; env probes were not in it, and the capture says they should have been.
//
// Measured on an interior capture (18506), per view, from the port's own pass markers:
//
//     VRCAM | ReflectionProbes   37 actions: 30+ draws of 3 indices into ResourceId::26530
//                                (512x256, THIRTY-TWO array slices, R11G11B10_FLOAT -- the probe atlas)
//                                with 1x1x1 dispatches interleaved, i.e. per-probe convolution
//     MAIN  | ReflectionProbes    1 action:  a single 80x80 dispatch and two copies. No probe rendering.
//
// So the second view rebuilds the SHARED probe atlas every frame and MAIN does not -- the same shape as the
// local-shadow thrash that this file already documents: a fresh view has nothing cached, so it forces a full
// re-render into a cache MAIN relies on. Everything else in the specular path is symmetric (DrawConeAO 5/5,
// PrepareFeedbackSSRBuffer_PreSSR 21/21, ScreenSpaceReflections 5/5), which makes this the only candidate left
// for the symptom: interior lighting differs between the eyes, "shiny in VRCAM, normal in MAIN". A probe that
// has just been rasterised and not yet convolved reads as a mirror rather than a rough reflection, which is
// what "shiny" looks like.
//
// Same ABI as the distant and local skips: a2 = rdx, view ctx = *(a2+0x18), key @ ctx+0x28, and the skipped
// node returns 0 exactly as RenderLocalShadowMaps does.
//
// NOT PROVEN, and the failure mode is written down first: if the second view genuinely needs its own probes,
// its reflections go flat or stale instead of shiny, and this goes back to 0. Two earlier suspects for this
// same symptom were eliminated by measurement -- RenderLightsIntegrate (identical bindings, identical output
// statistics) and local shadow reuse (tested at 0, artefacts unchanged).
// MEASURED HARMFUL AT 1, AND KEPT AT 0. Turning it back off fixed two things the user had reported as part of
// the interior bug: the glasses on the bar went from bluish back to the same yellow MAIN shows, and the black
// patch on the sofa chairs disappeared. Both of those were MINE -- introduced by this knob, not by the port's
// stereo path -- and they were mixed into the symptom list for an hour before that came out.
//
// The mechanism is the one the capture had already shown and I armed the knob against anyway: the SECOND VIEW
// is the one that FILLS the shared reflection-probe atlas (ResourceId::26530, 512x256 x32 slices -- 30 draws
// in VRCAM's turn against none in MAIN's). Skipping its node does not make it reuse MAIN's work, because MAIN
// does no probe work; it leaves the atlas to nobody. Unconvolved or stale probe faces read as a mirror and as
// black, which is exactly what was seen. Reuse of a cache only helps when someone else fills it.
//
// A separate lesson, worth more than the knob: DO NOT TREAT AN x64dbg READ AS A MEASUREMENT WITHOUT CHECKING
// debug_get_state FIRST. DebugProbeSkipHits read as 0 with the debugger detached (state "stopped"), so the
// expression resolved against the image instead of the process, and that zero was reported as evidence. The
// log is the instrument that does not lie about this; the detour now reports itself there.
//
// The detour now reports itself into the log, which needs no debugger: entries at zero means this detour is
// not on the path at all; entries climbing with match=0 means the ABI assumption is wrong and the view
// context is not at *(a2+0x18). The view KEY itself is known good -- the sky-node probe below reads
// *(ctx+0x28) the same way and separates the views correctly thousands of times a run.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_ProbeReuseMode = 0;   // 1=skip vrcam's rebuild, 0=off
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugProbeSkipHits = 0;

using ReflectionProbesFn = __int64(__fastcall*)(void*, void*);
static ReflectionProbesFn g_orig_reflection_probes = nullptr;

static bool probes_is_vrcam(void* a2) {
    if (CyberpunkVR_ProbeReuseMode != 1 || !a2) return false;
    __try {
        uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
            reinterpret_cast<uint8_t*>(a2) + 0x18);
        return ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugProbeNodeEntries = 0;

static __int64 __fastcall Detour_ReflectionProbes(void* a1, void* a2) {
    InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugProbeNodeEntries));
    // One line the first time through, whatever the launcher's DEBUG box says: a skip counter stuck at zero has
    // two completely different causes and this is the cheapest way to tell them apart. Entries at zero means the
    // detour is not on the path; entries climbing with key != vrcam means the ABI assumption is wrong.
    static std::atomic<int> said{0};
    if (said.exchange(1) == 0) {
        uintptr_t ctx = 0, key = 0;
        __try {
            ctx = a2 ? *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(a2) + 0x18) : 0;
            if (ctx) key = *reinterpret_cast<uint64_t*>(ctx + 0x28);
        } __except (EXCEPTION_EXECUTE_HANDLER) { }
        log("[probes] first entry: a2=%p ctx=*(a2+0x18)=%p key=%llX vrcamKey=%llX match=%d mode=%u",
            a2, (void*)ctx, (unsigned long long)key, (unsigned long long)g_vrcam_ctx_key,
            (ctx && key == g_vrcam_ctx_key) ? 1 : 0, CyberpunkVR_ProbeReuseMode);
    }
    // The repeating half: only while the knob is armed, so an ordinary session with it off stays silent after
    // the one-shot line above.
    if (CyberpunkVR_ProbeReuseMode) {
        static uint64_t s_last = 0;
        const uint64_t now = GetTickCount64();
        if (!s_last || now - s_last >= 5000) {
            s_last = now;
            log("[probes] node entries=%llu skips=%llu mode=%u",
                (unsigned long long)CyberpunkVR_DebugProbeNodeEntries,
                (unsigned long long)CyberpunkVR_DebugProbeSkipHits, CyberpunkVR_ProbeReuseMode);
        }
    }
    if (probes_is_vrcam(a2)) { ++CyberpunkVR_DebugProbeSkipHits; return 0; }
    return g_orig_reflection_probes(a1, a2);
}

static __int64 __fastcall Detour_LocalShadowMaps(void* a1, void* a2) {
    if (local_shadow_is_vrcam(a2)) { ++CyberpunkVR_DebugLocalShadowSkipHits; return 0; }
    return g_orig_local_shadow(a1, a2);
}

static bool gi_node_is_vrcam(void* a2) {
    if (CyberpunkVR_GiReuseMode != 1 || !a2) return false;
    __try {
        uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
            reinterpret_cast<uint8_t*>(a2) + 0x18);
        return ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
static char __fastcall Detour_GiNode(void* a1, void* a2) {
    if (!gi_node_is_vrcam(a2)) return g_orig_gi_node(a1, a2);
    __try {
        uintptr_t ctx = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(a2) + 0x18);
        auto earlychk = reinterpret_cast<char(__fastcall*)(void*)>(g_exe_base + GI_EARLYCHK_RVA);
        char result = earlychk(reinterpret_cast<void*>(ctx + 0x14));
        if (result) return result;                          // engine early-out
        uintptr_t renderer = *reinterpret_cast<uintptr_t*>(g_exe_base + RENDERER_GLOBAL_RVA);
        if (renderer) {
            uintptr_t applyMgr = *reinterpret_cast<uintptr_t*>(renderer + 0xC0);
            if (applyMgr) {                                 // SKIP update (122 build); apply only
                ++CyberpunkVR_DebugGiSkipHits;
                auto apply = reinterpret_cast<char(__fastcall*)(void*, void*)>(g_exe_base + GI_APPLY_RVA);
                return apply(reinterpret_cast<void*>(applyMgr), a2);
            }
        }
        return 0;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return 0;   // safe: skip GI this frame for vrcam rather than crash
    }
}

// ================================================================================================
// THE FOLIAGE WIND IMPULSE -- one write, and the measurement behind it.
//
// Symptom (2026-08-17): standing in a bush made it twitch, and turning the head made all of MAIN's
// vegetation jitter while the second eye stayed perfect. The decisive datum came from the user:
// switch the SECOND view off and MAIN is correct. So MAIN was never short of data -- it was affected
// by the mere existence of the other view.
//
// THE PASS. CRenderNode_WindImpulseVolumeUpdate (sub_1406EAEDC) is the only thing that injects the
// repellers -- actors standing in vegetation, gathered by the game-side tick group the exe names
// "RepellerComponents/FoliageRepeller" -- into the volume the foliage shaders read. It decides
// whether to do anything from two gates:
//
//     windState = *(*(wc + 0x20) + 0xB0)                      // ONE object, shared by both views
//     if ((wc[0x30] & 2) == 0)                       -> skip  // not the view that owns shared work
//     if (!latch(windState + 0x1C, renderer+0x4CA4)) -> skip  // already done this frame
//
// The latch is an InterlockedExchange of the frame id, so EXACTLY ONE view per frame gets through --
// correct for a game with one view, and the whole of this bug for two. Measured in x64dbg: the body
// is entered by VRCAM (view key at ctx+0x28 non-zero) in 5433 frames out of 5791, while MAIN arrives
// with the owner bit set, finds the frame already claimed, and jumps over everything.
//
// AND THE ANSWER IS NOT WHAT THE VOLUME CONTAINS BUT WHEN IT IS WRITTEN. Each of these was built,
// measured to reach MAIN, and left the jitter exactly where it was:
//
//   * replaying the pass's 48-byte constant buffer for MAIN, captured in the uploader, same frame
//     (2987 extra runs, 2988 replays, 0 stale) -- MAIN still jittered;
//   * rebuilding those 48 bytes from the shared state instead of copying them -- and a self-check
//     against the engine's own upload showed the built block differing by exactly one scroll step,
//     because the state advances between the two dispatches. Fresh is the wrong thing to be here:
//     the volume was written with the OTHER view's numbers;
//   * replaying the pass's five resource bindings (51656 recorded, 4119 replayed);
//   * reopening the same latch on CRenderNode_AdvanceSpeedTreeWind, the global wind (4121 extra runs);
//   * running the whole pass for both views with the clock restored, the ageing suppressed and the
//     impulse records snapshotted and put back -- which made BOTH eyes twitch and cost frame rate.
//
// That last one produced the trace worth keeping. A hardware write breakpoint on the impulse count at
// windState+0x104 caught two writers:
//
//     0x6EDB36  mov  [rdi+0x0C], esi     the game ADDS an impulse (esi=1, capacity 0xFF)
//     0x6EE158  dec  [rcx+0x0C]          the pass RETIRES one, and MOVES records to close the gap
//
// so restoring the count alone re-injects a record that has been shuffled -- and restoring the bytes
// as well still twitches, because the second dispatch itself is what one shared volume cannot
// survive.
//
// WHAT IT ACTUALLY IS. The claim is taken on the CPU by whichever view's node runs first, but the two
// graphs do not reach the GPU in that order: MAIN's draws are submitted before the second view's
// dispatch, so MAIN samples the volume as of the PREVIOUS frame. Correct data, one frame old --
// invisible standing still, and exactly a jitter while the head turns. Switching the second view off
// makes MAIN claim the latch itself, which puts its dispatch ahead of its own draws, which is why
// that looked right.
//
// THE FIX is to hand the claim to MAIN: deny it to the second view by pre-claiming the frame, grant it
// to MAIN by rewinding one. One update per frame, no second dispatch, no frame-rate cost, and the view
// that draws later reads the same fresh volume. Confirmed by the user: no difference between the eyes.
//
// `CyberpunkVR_FoliageWindMode`: 1 = this, 0 = the engine's own behaviour. Live-flippable, which is how
// all of the above was walked.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_FoliageWindMode = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugWindClaimMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugWindDenyVrcam = 0;

using WindNodeFn = char(__fastcall*)(void*, void*);
static WindNodeFn g_orig_wind_node = nullptr;

static char __fastcall Detour_WindImpulseNode(void* a1, void* a2) {
    if (CyberpunkVR_FoliageWindMode != 1 || !a2 || !g_exe_base) return g_orig_wind_node(a1, a2);
    uint8_t* wc = reinterpret_cast<uint8_t*>(a2);
    __try {
        // Only the owner dispatches reach the gates at all; the rest are the engine's own no-ops.
        if ((wc[0x30] & 2) == 0) return g_orig_wind_node(a1, a2);
        const uintptr_t holder = *reinterpret_cast<uintptr_t*>(wc + 0x20);
        const uintptr_t wind = holder ? *reinterpret_cast<uintptr_t*>(holder + 0xB0) : 0;
        const uintptr_t renderer =
            *reinterpret_cast<uintptr_t*>(g_exe_base + RENDERER_GLOBAL_RVA);
        if (!wind || !renderer) return g_orig_wind_node(a1, a2);
        const uint32_t rframe = *reinterpret_cast<uint32_t*>(renderer + 0x4CA4);
        const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(wc + 0x18);
        const bool vrcam = ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
        // The pass's own InterlockedExchange leaves the frame id behind either way, so no other
        // reader ever sees the rewound value.
        *reinterpret_cast<volatile uint32_t*>(wind + 0x1C) = vrcam ? rframe : (rframe - 1);
        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
            vrcam ? &CyberpunkVR_DebugWindDenyVrcam : &CyberpunkVR_DebugWindClaimMain));
    } __except (EXCEPTION_EXECUTE_HANDLER) { }
    return g_orig_wind_node(a1, a2);
}


// ================================================================================================
// THE OTHER WIND -- and the one that actually moves the vegetation.
//
// Everything above concerns the IMPULSE volume: the local push of an actor standing in a bush. It is
// now measured not to be the twitch: MAIN received the engine's own same-frame constant block every
// frame (2987 extra runs, 2988 replays, 0 stale) and still twitched on head turns. So that thread is
// closed, and the symptom -- ALL of MAIN's grass, not just the bush the player stands in -- points at
// the global wind instead.
//
// CRenderNode_AdvanceSpeedTreeWind (sub_140CC4DF4) is three lines and both of this project's usual
// suspects are in them:
//
//     if ((wc[0x30] & 2) != 0)                          // the owner bit
//         return sub_140200B4C(*(*(wc + 0x20) + 0x80));  // the SpeedTree wind manager
//   and inside:
//     if (latch(mgr + 0x48, renderer + 0x4CA4))          // the same once-per-frame latch
//         for each wind instance: push the current wind, then sub_1401F088C(idx, 416, inst + 0x4538)
//
// So one view per frame uploads 416 bytes of wind state and the other view's vegetation is left with
// whatever was last bound to it -- stale, which is what "the grass freezes when I turn my head" is.
//
// AND THIS ONE IS SAFE TO RUN TWICE, which is the whole difference from the impulse pass. It
// accumulates nothing and consumes nothing: the values come from the manager (`v4+2848` scaled, and
// the three dwords at `v4+2864..2872`), are written into each instance and uploaded. Run it again in
// the same frame with the same inputs and it writes the same bytes. So the fix is only to reopen the
// latch for the second view -- no snapshot, no restore, nothing held.
//
// AND 2026-08-17 GAVE IT A SECOND, SHARPER REASON: the shadows cast by fences, plants and trees differ
// between the eyes, while solid objects' shadows match -- so whatever it is acts only on geometry that
// MOVES. The cascade pixel shader's three discards are all eliminated by measurement (the LOD-fade phase
// reads 0 in both views with one distinct value seen; forcing the blue-noise slice to a constant in
// every frame-constants block changed nothing; neutralising the height-mask test changed nothing), and
// the cascade VERTEX shader sways foliage in the shadow pass itself out of b8 -- a wind-texture scroll
// at b8[4].xy, a phase at b8[5].w. b8 is 26 float4 = 416 bytes, exactly what this pass uploads, and the
// census names the owner: AdvanceSpeedTreeWind, 29638 uploads for MAIN and ZERO for the second view.
//
// One buffer, written once per frame, read by two graphs at different times. The second view's graph
// runs FIRST (measured: its passes at capture event 12909, MAIN's at 40420), so every frame goes
//
//     second view's cascades   read the buffer as it stands  -> wind(N-1)
//     AdvanceSpeedTreeWind     writes it                     -> wind(N)
//     MAIN's cascades          read it                       -> wind(N)
//
// and the second eye's shadow map holds its foliage one wind step behind its own image. Invisible on
// anything rigid, unmistakable on leaves. Reopening the latch puts the upload in the second view's graph
// too, ahead of its cascades.
//
// Recorded because it nearly cost the finding: this latch was reopened once before, during the bush
// jitter hunt, and "changed nothing" -- true of THAT symptom, and the shadows were not being looked at.
// A knob measured innocent of one fault is not innocent of the next.
// THE WIND IS BACK IN THE FRAME, and my earlier reading of these counters was wrong. From
// First = 8441, ExtraMain = 5870, ExtraVrcam = 0 I concluded "the second view claims the wind, so both
// eyes read the same one". That skipped the fact this hunt had already established elsewhere: the
// renderer's frame id ADVANCES BETWEEN THE TWO VIEWS' PASSES. The latch compares against that id, so each
// view sees it as a new frame and each claims the wind for itself:
//
//     second view's frame:  latch = its id     -> uploads wind(N)
//     MAIN's frame:         id has moved on    -> claims it too, uploads wind(N+1)
//
// so the grass in the two shadow maps sways one wind step apart. 8441 claims in an interval is TWO per
// eye-pair, not one. And it fits what the user sees now: with everything else settled the cable's shadow
// (rigid) matches, while grass (wind-animated) still differs.
//
//   mode 0  the engine's own behaviour: each view advances the wind in its own frame
//   mode 1  reopen the latch for the view that finds it claimed (idempotent, but pointless -- it just
//           re-uploads the same values)
//   mode 2  DENY the second view the claim, so one wind state serves the pair
//
// Default 0 deliberately: mode 2 is the candidate and it is switched on live, one variable at a time,
// because testing it together with the block change above would be two changes and no measurement.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_SpeedTreeWindMode = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugStWindFirst = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugStWindExtraMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugStWindExtraVrcam = 0;

using SpeedTreeWindFn = int64_t(__fastcall*)(void*, void*);
static SpeedTreeWindFn g_orig_st_wind = nullptr;

static int64_t __fastcall Detour_SpeedTreeWind(void* a1, void* a2) {
    if (CyberpunkVR_SpeedTreeWindMode == 0 || !a2 || !g_exe_base) return g_orig_st_wind(a1, a2);
    uint8_t* wc = reinterpret_cast<uint8_t*>(a2);
    __try {
        if ((wc[0x30] & 2) == 0) return g_orig_st_wind(a1, a2);   // not an owner call: untouched
        const uintptr_t holder = *reinterpret_cast<uintptr_t*>(wc + 0x20);
        const uintptr_t mgr = holder ? *reinterpret_cast<uintptr_t*>(holder + 0x80) : 0;
        const uintptr_t renderer =
            *reinterpret_cast<uintptr_t*>(g_exe_base + RENDERER_GLOBAL_RVA);
        if (!mgr || !renderer) return g_orig_st_wind(a1, a2);
        const uint32_t rframe = *reinterpret_cast<uint32_t*>(renderer + 0x4CA4);

        // MODE 2: one wind state per eye-pair. The second view is handed the latch already claimed, so it
        // skips the advance and reads what the first view uploaded. The first view is left alone, so the
        // wind still advances once per pair rather than freezing.
        if (CyberpunkVR_SpeedTreeWindMode == 2) {
            const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(wc + 0x18);
            const bool vrcam = ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
            // The second view is MAIN: the capture puts the other view's passes first every frame.
            if (!vrcam) {
                *reinterpret_cast<volatile uint32_t*>(mgr + 0x48) = rframe;
                InterlockedIncrement64(
                    reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugStWindExtraMain));
            } else {
                InterlockedIncrement64(
                    reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugStWindFirst));
            }
            return g_orig_st_wind(a1, a2);
        }

        if (*reinterpret_cast<volatile uint32_t*>(mgr + 0x48) != rframe) {
            InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugStWindFirst));
        } else {
            // Reopen for this call only; the callee's own InterlockedExchange writes the frame id
            // straight back, so nothing else ever sees the reopened value.
            *reinterpret_cast<volatile uint32_t*>(mgr + 0x48) = rframe - 1;
            const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(wc + 0x18);
            const bool vrcam = ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
            InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
                vrcam ? &CyberpunkVR_DebugStWindExtraVrcam : &CyberpunkVR_DebugStWindExtraMain));
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) { }
    return g_orig_st_wind(a1, a2);
}

// ================================================================================================
// THE CASCADE'S OWN MATRIX, keyed by cascade index -- the last unverified assumption.
//
// The thin-shadow report (a cable on a building differs between the eyes) overturned my own reason for
// excluding the cascade transform. I had argued that a placement difference would move EVERY shadow;
// that is wrong. A cascade texel is a few centimetres and the eyes are 6.5 cm apart, so an offset of a
// texel is invisible on a building's broad shadow and decisive on anything thinner than a few texels:
// a cable, a fence slat, a branch. Which is exactly the set of objects reported.
//
// What IS verified, live in the debugger: the two views' cascade passes reach the same scene manager --
// *(view_ctx + 0x1E10) reads 0x2A926A675C0 for both -- so the cascade records are one shared object, not
// one per view. But a shared pointer is not a shared VALUE at draw time: this project's own notes record
// the pattern of two views rebuilding one global in turn, and if each view writes its placement before
// its own pass, both read "their own" matrix out of the same memory.
//
// So the matrix has to be compared AT THE MOMENT EACH VIEW DRAWS, and keyed by cascade index -- the
// mistake that wasted three measurements today was keying too coarsely, and the cascade node uploads its
// 848-byte block two dozen times a frame. The index is taken from the node argument the pass itself uses
// (a1 + 24, the same read as `v12 = *(unsigned int *)(a1 + 24)`) and published in a thread-local for the
// upload probe to key on.
// AND THE FIELD THAT MAKES THEM DIFFER, found in the pass's own body after the plugin captured the call
// stack at the upload (engine frames 1E37E3 153A6C 1EC46D ... -- 153A6C is inside this very node, so the
// constants are built here rather than by a separate setup):
//
//     sub_14028DB28(v51, v13 + 801616);     // v51 = local cascade constants, from the SHARED record
//     v20 = *(float *)(v14 + 2664);         // v14 = viewData + 16 * cascade_idx  -> PER VIEW
//     v52 = v52 - v20;                      // two fields of the constants, adjusted by it
//     v53 = v53 - v20;
//     sub_1401E2C94(a2, (__int64)v51, ...); // uploaded
//
// So the cascade record itself is shared -- the pointer was verified identical live -- and what splits
// the two eyes is one float per cascade taken from each view's own viewData (the getter is sub_1401ED930,
// which this port already calls elsewhere). Equalising THAT is the fix, and it has to be done in the
// viewData rather than in the upload: the lighting pass samples with the matrix its view rendered with,
// which is why lending MAIN's matrix to the second view's upload made grass slide with head turns.
//
// Not restored afterwards, deliberately: the value must still read as MAIN's when the sampling side comes
// to it later in the frame. Each view has its own viewData, so writing the second view's cannot disturb
// MAIN, and the engine recomputes it next frame.
// Stays ON: it only publishes the cascade index in a thread-local, which costs one store per cascade
// node and is what any further work on this will need to key by.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CascadeIdxProbe = 1;
// The per-index dispatch count. Default 0 now that it has answered; see the note at its call site.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CascCountProbe = 0;
// DEFAULT 0: measured to read 0.060000 in both views, so it is a shared constant and lending it does
// nothing. Kept only so the measurement can be repeated.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CascadeBiasLend = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCascBiasLends = 0;
thread_local int32_t t_cascade_idx = -1;

// ---- THE CASCADE RECORD, and which of its fields split the eyes ----------------------------------
//
// The viewData field above was a dead end: it reads 0.060000 in BOTH views, so it is a shared constant
// (a depth bias) and never lent anything. What actually differs was found by dumping the record itself at
// each view's pass -- scene_mgr + 801616 + 1104 * cascade_idx, the block the pass copies its constants
// from, at the shared pointer verified live:
//
//     +0x10  orientation quaternion            IDENTICAL in both views (the sun direction is shared)
//     +0x40  two floats, the cascade extent    VRCAM -18.0587 / 18.0587   MAIN -18.0586 / 18.0586
//     +0x80  three floats, the cascade CENTRE  VRCAM (-1959.021, -363.2735, -1442.0868)
//                                              MAIN  (-1958.997, -363.2810, -1442.1367)
//
// THOSE OFFSETS ARE 0x40 AND 0x80, and the first attempt wrote 0x50 and 0x90 -- I had misread the first
// dump by one sixteen-byte row. At 0x50 and 0x90 sit direction vectors, so the "fix" handed each view the
// other's directions, and the bushes' shadows swung with the head. Re-read row by row against a fresh
// dump before believing an offset; the failure was arithmetic, not the approach.
//
// AND THEN THE IN-FRAME PROBE RETRACTED ALL OF IT. Those numbers above came from TWO DEBUGGER STOPS, in
// different frames, and in VR the head never holds still -- so they are a measurement of head jitter, not
// of the two views. Running the same comparison inside one frame, from the detour, reports:
//
//     [cascrec] cascade 1 record, dwords differing between the views (0): (none)
//
// zero differing dwords across the whole 1104-byte record, repeatedly. The record is SHARED and IDENTICAL:
// both eyes place the cascade the same way, and the "5 cm centre offset" never existed. The two failed
// lends (this one and the deleted CascMatFix) were therefore fixing nothing and breaking the render/sample
// agreement of whichever view received the copy.
//
// The rule this cost: a cross-frame comparison cannot answer a per-view question. Two breakpoint hits are
// two different frames. Only a probe that holds both views' copies from ONE frame may be believed -- and
// that is also how the same trap was caught with the 480-byte block earlier the same day.
//
// AND THE "RECORD IDENTICAL" READING IS NOW RETRACTED FOR THE SECOND TIME, by a probe that measures instead
// of asking yes/no. With the head turning, the two views fit cascade centres 9-13 cm apart on cascade 0 and
// 21-36 cm apart on cascade 1 -- five to seven texels and more, in one frame. Standing still they agree,
// which is why a boolean taken at rest said "identical" and why the user's "only when I turn my head" was the
// observation that broke the case open.
//
// Both eyes write that one shared atlas from boxes up to a third of a metre apart, so their content is
// quantised on grids several texels offset. A shadow one or two texels wide lands on a different texel in
// each eye (grass), and along a view ray in the volumetric fog the offset accumulates into a visible
// horizontal displacement, opposite in the two eyes (the blocky sun shafts in the hospital-bed ending, which
// the user reports as far more visible than the grass -- same defect, amplified by the ray).
//
// ON, with the fit copied whole. This is the first version aimed at the measured cause rather than at a
// guess, and CyberpunkVR_CascFitProbe is the check: if the lend works, maxCentre must fall to zero.
//
// AND THE WARNING TWENTY LINES UP CAME TRUE, WHICH IS WHY THIS LEND NEEDS A PARTNER. "Breaking the
// render/sample agreement of whichever view received the copy" is exactly what the user then reported:
// shadows blinking in MAIN, clean in the second view -- MAIN being the view that receives the copy. A capture
// measured the broken half directly: MAIN rasterises with the second view's fit (this lend, [cascfit]
// after=0.0000 m) and still SAMPLES with a matrix built from its own camera, and the two disagree by 58.1 mm
// along the light -- the 0.0640 m eye separation projected onto the sun direction, reproduced to 0.3 mm in two
// independent captures.
//
// The sampling matrix is not in this record, which is why zeroing the record's difference could never fix it:
// it lives in the 928-byte CSConstants block the frame graph uploads at PrepareSceneRendering.
// CyberpunkVR_CascSampleLend in Grading.cpp lends it in the same direction as this one. The two belong
// together -- lending either alone leaves one view rasterising and sampling on different grids.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CascRecLend = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCascRecLends = 0;

namespace {
constexpr uintptr_t kCascRecBase = 801616;       // scene_mgr + 801616 + 1104 * idx
constexpr uintptr_t kCascRecStride = 1104;
constexpr uint32_t  kCascRecScan = 1104;         // how much of it to diff for the report
// The groups measured to differ and safe to copy: the extent pair and the centre triple.
constexpr uint32_t kCascExtentOff = 0x40;
constexpr uint32_t kCascExtentLen = 8;
constexpr uint32_t kCascCentreOff = 0x80;
constexpr uint32_t kCascCentreLen = 12;

uint8_t g_casc_rec[8][kCascRecScan] = {};
bool    g_casc_rec_have[8] = {};

constexpr uintptr_t kCascBiasOffset = 2664;      // viewData + 2664 + 16 * cascade_idx
float g_casc_bias_main[8] = {};
bool  g_casc_bias_have[8] = {};
// Its own pointer to the viewData getter rather than the file's shared one, which is declared further
// down: sub_1401ED930, the same function the pass itself calls.
using CascViewDataFn = uintptr_t (__fastcall*)(void*);
CascViewDataFn g_casc_viewdata = nullptr;

// ---- HOW FAR APART THE TWO VIEWS FIT THE CASCADE, in world units ---------------------------------
//
// The user's observation is what this exists for, and it is the sharpest evidence of the day: with the reuse
// armed, the shadows are CORRECT while the head is still and wrong the moment the head TURNS. Translation
// does not do it; rotation does. A sun cascade is fitted to the viewer -- centre roughly position plus
// forward times half the range -- so orientation moves the centre by metres where position moves it by
// centimetres. Cascade 0's extent is +-18 world units, a 36 m box over maybe 2048 texels, i.e. ~1.8 cm per
// texel: a couple of degrees of head turn is hundreds of texels.
//
// So the two views do NOT fit the same cascade whenever the head is moving, and reuse fails for that reason
// alone -- no barrier, no ordering, no second texture required to explain it.
//
// AND THE EARLIER "0 differing dwords" HAS TO BE RE-READ IN THAT LIGHT. It was a yes/no over the record
// bytes, taken while standing still, and it answered "are they ever different" with the head not moving.
// This reports the MAGNITUDE instead, and its maximum over the interval, so head motion cannot hide inside a
// boolean. If the centres separate by centimetres or more while turning, the same mechanism is a candidate
// for the ORIGINAL defect too: two eyes whose shadow maps are fitted to poses sampled a few milliseconds
// apart would disagree exactly where the shadow is a texel or two wide.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_CascFitProbe = 1;
namespace {
struct CascFit {
    float centre[3];
    float extent[2];
    bool  have;
    float max_centre_d;
    float max_extent_d;
    float max_pre_d;        // ...as the ENGINE fitted it, before the lend touched anything
    uint64_t pairs;
};
CascFit g_cascfit[8] = {};

float cascfit_centre_delta(const uint8_t* a, const float* b) {
    float c[3] = {};
    __try { memcpy(c, a + kCascCentreOff, sizeof(c)); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return 0.0f; }
    const float dx = c[0] - b[0], dy = c[1] - b[1], dz = c[2] - b[2];
    return sqrtf(dx * dx + dy * dy + dz * dz);
}

// The separation the ENGINE produced, sampled at MAIN's pass just before the lend overwrites it. Without
// this the post-lend zero is unreadable: zero also happens when the head is still and the two views agreed
// anyway, which is exactly the trap the boolean version of this probe already fell into once.
void cascfit_note_pre(const uint8_t* rec_main, int32_t idx) {
    if (idx < 0 || idx >= 8) return;
    CascFit& f = g_cascfit[idx];
    if (!f.have) return;
    const float d = cascfit_centre_delta(rec_main, f.centre);
    if (d > f.max_pre_d) f.max_pre_d = d;
}

void cascfit_note(const uint8_t* rec, int32_t idx, bool vrcam) {
    if (idx < 0 || idx >= 8) return;
    float centre[3] = {}, extent[2] = {};
    __try {
        memcpy(centre, rec + kCascCentreOff, sizeof(centre));
        memcpy(extent, rec + kCascExtentOff, sizeof(extent));
    } __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    CascFit& f = g_cascfit[idx];
    // The second view runs first, so it records and MAIN compares -- which makes the comparison in-frame by
    // construction, with no frame id to key on (the renderer's advances between the two views' passes).
    if (vrcam) {
        memcpy(f.centre, centre, sizeof(centre));
        memcpy(f.extent, extent, sizeof(extent));
        f.have = true;
        return;
    }
    if (!f.have) return;
    const float dx = centre[0] - f.centre[0];
    const float dy = centre[1] - f.centre[1];
    const float dz = centre[2] - f.centre[2];
    const float dc = sqrtf(dx * dx + dy * dy + dz * dz);
    const float de = fabsf(extent[0] - f.extent[0]) + fabsf(extent[1] - f.extent[1]);
    if (dc > f.max_centre_d) f.max_centre_d = dc;
    if (de > f.max_extent_d) f.max_extent_d = de;
    ++f.pairs;
}

void cascfit_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 3000) return;
    s_last = now;
    char line[420];
    int used = 0;
    line[0] = 0;
    for (int i = 0; i < 8; ++i) {
        if (!g_cascfit[i].pairs) continue;
        if (used < static_cast<int>(sizeof(line)) - 70)
            used += snprintf(line + used, sizeof(line) - used,
                             "casc%d engine=%.4f m after=%.4f m maxExtent=%.4f pairs=%llu  ", i,
                             g_cascfit[i].max_pre_d, g_cascfit[i].max_centre_d,
                             g_cascfit[i].max_extent_d,
                             (unsigned long long)g_cascfit[i].pairs);
        // Per interval, so the number describes the seconds being looked at rather than the whole session.
        g_cascfit[i].max_centre_d = 0.0f;
        g_cascfit[i].max_extent_d = 0.0f;
        g_cascfit[i].max_pre_d = 0.0f;
        g_cascfit[i].pairs = 0;
    }
    log("[cascfit] centre separation, worst in this interval -- engine = as fitted, after = after the lend: %s",
        used ? line : "(no pair seen)");
}
}  // namespace

// Every differing dword in the record, so a field I have not accounted for cannot hide. Throttled, and it
// skips the first sixteen bytes: those are allocation handles and differ by design.
void casc_rec_report(int32_t idx, const uint8_t* mine, const uint8_t* theirs) {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    char line[900];
    int used = 0, n = 0;
    line[0] = 0;
    for (uint32_t o = 16; o + 4 <= kCascRecScan; o += 4) {
        if (memcmp(mine + o, theirs + o, 4) == 0) continue;
        ++n;
        float a = 0.0f, b = 0.0f;
        memcpy(&a, mine + o, 4);
        memcpy(&b, theirs + o, 4);
        if (used < static_cast<int>(sizeof(line)) - 60)
            used += snprintf(line + used, sizeof(line) - used, "+%X %.4f/%.4f  ", o, a, b);
    }
    log("[cascrec] cascade %d record, dwords differing between the views (%d): %s || lends=%llu",
        idx, n, n ? line : "(none)", (unsigned long long)CyberpunkVR_DebugCascRecLends);
}

// HOW MANY CASCADES EACH VIEW ACTUALLY RENDERS, counted per index.
//
// In gameplay -- and only in gameplay, the earlier reading was taken in a menu and is withdrawn -- the
// small-block census shows the cascade node and the procedural grass scatter both running about 5.2x more
// often for MAIN than for the second view, the same ratio for both. Grass is what the user sees differing.
// And the matrix probe only ever reported cascades 0 and 1 on both sides, never 2 or 3.
//
// The earlier draw census could not have caught this: it counted draws in total, not per cascade, so a
// cascade the second view skips entirely hides inside a number that looked balanced (94002 against 94004).
uint64_t g_casc_dispatch[2][8] = {};

void casc_count_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    char line[400];
    int used = 0;
    line[0] = 0;
    for (int i = 0; i < 8; ++i) {
        if (!g_casc_dispatch[0][i] && !g_casc_dispatch[1][i]) continue;
        if (used < static_cast<int>(sizeof(line)) - 40)
            used += snprintf(line + used, sizeof(line) - used, "casc%d M=%llu V=%llu  ", i,
                             (unsigned long long)g_casc_dispatch[0][i],
                             (unsigned long long)g_casc_dispatch[1][i]);
    }
    log("[casccount] cascade-node dispatches per index and view: %s", used ? line : "(none)");
}

void casc_bias_report(int32_t idx, float m, float v) {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    log("[cascbias] viewData+%u+16*%d : MAIN %.6f | VRCAM %.6f | delta %.6f | lends=%llu",
        (unsigned)kCascBiasOffset, idx, m, v, v - m,
        (unsigned long long)CyberpunkVR_DebugCascBiasLends);
}
}  // namespace

// ---- CASCADE REUSE, POINTED THE RIGHT WAY --------------------------------------------------------
//
// The frame-graph route (CyberpunkVR_VrcamFlagMode = 2, feature bit 50) gave the SECOND view the reuse, and
// it produced sliding shadows in that eye: its graph runs FIRST, so it sampled an atlas rasterised the
// previous frame while using this frame's matrix.
//
// That failure also settled a question the flag comment only implied: the eye that skipped its build still
// HAD shadows -- wrong ones, not missing ones -- so the cascade atlas is one shared resource, not a texture
// per graph. Which means reuse works in the other direction with no staleness at all: let the second view
// (first to run) build the atlas, and skip the build for MAIN, which runs after it in the same frame. The
// in-frame record probe already showed the cascade record is identical at both passes, so MAIN's sampling
// matrix matches what the second view rasterised.
//
// Both nodes have to go for MAIN, not just the render: ClearShadowCascades would otherwise wipe the atlas
// between the second view's build and MAIN's sampling, and MAIN would lose sun shadows entirely. That is
// also the recognisable failure if this reasoning is wrong.
//
// Default 0. It is a single dword, so it can be armed live in the debugger without a rebuild, and its whole
// purpose is to answer one question: the two views' cascade INPUTS are measured identical, but their
// OUTPUTS never were. If MAIN's shadows change at all when it samples the other eye's atlas, they were not
// identical, and that is the foliage mismatch. If nothing changes, the atlases agree and the mismatch is
// downstream, in the half-resolution interleaved mask -- and this stays on anyway, because it takes a whole
// cascade rasterisation per frame off the GPU.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CascadeSkipMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCascadeMainSkips = 0;

// ---- THE SAME SAVING, WITHOUT TAKING THE BARRIERS WITH IT ----------------------------------------
//
// CascadeSkipMain above made MAIN's shadows TWITCH, and the likely reason is not that reuse is impossible
// but that it cut too much: skipping the RenderShadowCascade node removes the work AND everything the node
// does around it -- the render-target bind and the resource transitions the pass performs. A consumer then
// reads a texture whose state the graph believes to be something else.
//
// The depth-target probe says the saving is real and worth having: both views bind the SAME atlas
// descriptor per cascade (casc0/casc1 identical for MAIN and the second view) from an identical cascade
// record, so one of the two rasterisations is a pure duplicate -- roughly 47000 draws and 134000 instances
// a frame.
//
// So take the work and leave the structure:
//   * ClearShadowCascades: skipped at the NODE for MAIN, so the atlas the second view just filled survives.
//     The pass itself stays in the graph, so its edges and its ordering do not move.
//   * RenderShadowCascade: RUNS for MAIN -- every bind and transition it performs still happens -- and only
//     its DRAWS are suppressed, at the command list.
//
// The two knobs are deliberately separate: SkipMain is the measured-harmful version and stays at 0 as the
// record of what not to do; SaveMain is the version that keeps the barriers.
//
// AND IT FAILED IDENTICALLY -- "артефакты у main, мерцания, то же самое и при 50 бите" -- which is the most
// useful result of the three, because it eliminates the explanation I had built the knob on. The node ran,
// every bind and transition it declares happened, and the picture broke exactly as it did when the node was
// cut outright. So the barriers were never what was missing.
//
// THREE VARIANTS, AND THE USER CORRECTED ME ON THE MOST IMPORTANT DETAIL: it is ONE artefact, not three.
// I had written them up as "slides", "twitches" and "flickers" as though the failure modes differed and
// each pointed somewhere; they do not. Same artefact every time, which means one mechanism, and my three
// separate diagnoses were three inventions.
//
//   bit 50 on the second view   its graph emits no cascade passes at all
//   both nodes cut for MAIN     work and structure both gone
//   clear node kept, only the clear command and the draws dropped
//
// AND THE ONE MECHANISM IS HEAD ROTATION. With reuse armed the shadows are CORRECT while the head is still,
// and wrong the instant it turns -- the user's own test, and worth more than everything above. A sun cascade
// is fitted to the viewer, so orientation moves its centre by metres while position moves it by centimetres;
// cascade 0 covers a 36 m box, so a degree or two of turn is hundreds of texels. The eye that skips its own
// rasterisation therefore samples an atlas fitted to the pose of the OTHER view's pass, and those two poses
// are sampled milliseconds apart. Nothing about barriers, ordering or resource identity is required to
// explain any of the three failures.
//
// See CyberpunkVR_CascFitProbe above: it measures that separation in metres instead of asserting it, and if
// it is large while turning then the same mechanism is a candidate for the eye-difference itself.
//
// AND THE CAPTURE FOUND WHAT WAS ACTUALLY WRONG WITH IT, which was neither ordering nor the barriers the
// knob was built to protect. Reading the cascade lists' barriers with their STATES:
//
//     ClearShadowCascades  Resource_5159   NON_PIXEL_SHADER_RESOURCE -> DEPTH_WRITE      <- both runs
//     RenderCascade0/1     Resource_48841  COPY_DEST <-> VERTEX_AND_CONSTANT_BUFFER      <- run 0 only
//     RenderCascade0/1     Resource_4843   COPY_DEST <-> VERTEX_AND_CONSTANT_BUFFER      <- run 1 only
//
// The atlas is Resource_5159, ONE depth texture, barriered by both views. The two resources that differ are
// buffers -- their states say so -- i.e. each view's own constant/instance stream, nothing to do with the
// atlas. (I read that table wrong once and told the user there were two atlases; the states are what settle
// it, and they were in the same rows all along.)
//
// So the atlas IS shared, the second rasterisation IS redundant, and the reason this knob flickered is that
// it cut the ClearShadowCascades NODE -- taking with it the very transition above. MAIN then bound and
// sampled the atlas in a state the graph believed to be something else. The node cut and the "keep the
// barriers" variant failed identically because both removed that transition.
//
// SaveMain now withholds only the ClearDepthStencilView COMMAND, in hk_ClearDepthStencilView: the node runs,
// the transition happens, the depth the second view wrote this frame is what MAIN samples. SkipMain stays at
// 0 as the record of the cruder version.
//
// THE DUPLICATE IS REAL, AND HERE IS HOW IT WAS FINALLY LABELLED. Each view rasterises its own cascade block
// immediately before its own colour work, and a capture proves it WITHOUT markers, by reading whose camera is
// bound (CameraShaderConsts float 144), because the two eyes sit 0.0640 m apart in world X:
//
//     6061..6305    98 draws, camera buffer 3032    (546.0938, -2378.1738, 0)     cascade block 1
//     9000..12000   colour work, buffer 313090      (546.1180, -2378.1675, 174.8) view A
//     23772..23947  56 draws, camera buffer 3032    (546.0938, -2378.1738, 0)     cascade block 2
//     26000..30000  colour work, buffer 31650       (546.1820, -2378.1675, 174.8) view B
//
// 546.1820 - 546.1180 = 0.0640 m. Two views, each with its own camera buffer, each preceded by its own cascade
// block. So MAIN's pass IS a duplicate and withholding it saves real work -- measured live at 286 draws and one
// clear per frame, with the picture correct.
//
// AN EARLIER READING HERE WAS WRONG AND IS RETRACTED. Single frames showed one cascade block, or none, or two:
// the atlas update is SCHEDULED, so three consecutive frames gave 2, 0 (a copy instead) and 1 block. Counting
// blocks in one frame cannot establish who rasterises anything, which is how "the engine already does it once"
// got written down. The reading it replaced said:
//
//     23735..23879   forty depth-only DrawIndexedInstanced into the cascade target, ONE contiguous block,
//                    in the gap between the two views' graphs and immediately before MAIN's
//     234655         the atlas both masks sample -- FOUR usages in the whole frame, every one a READ
//                    (11447 and 15699 for one eye, 29723 and 34188 for the other), no write, no copy
//     and there is no second candidate: two 2048^2 R16 textures in the capture and no D16 texture at all
//
// The earlier capture agrees from the other side: of its nine ClearDepthStencilView events, eight came in
// per-eye pairs and exactly ONE was unpaired -- the cascade clear. (That pairing argument is also void for the
// same reason: it counted one frame.)
//
// AND NOTE WHY THE CAPTURE STILL SHOWS MAIN'S BLOCK WITH THIS KNOB ON. Our command-list hooks sit BELOW
// RenderDoc's wrapper: the game calls the wrapper, RenderDoc serialises the draw, and only then does our hook
// on the real list drop it. A capture therefore records what the GAME asked for, never what the GPU received --
// so nothing this knob does can be judged from a capture. The counters are the instrument.
//
// So "both views bind the same atlas from an identical record, therefore one of the two rasterisations is a
// pure duplicate" was reading two NODE invocations as two rasterisations. The node runs per view; the drawing
// happens once.
//
// ARMED AGAIN ANYWAY, ON THE USER'S POINT, WHICH IS THE BEST ARGUMENT IN THIS WHOLE FILE. Every failure above
// was diagnosed BEFORE the cascade camera was known to differ between the views. Each attempt changed who
// rasterises the atlas while leaving the camera and the sampling matrix per-view -- so every one of them could
// have been failing for the reason the three lends have now fixed, and "reuse is impossible" was never
// established, only assumed from the wreckage.
//
// This knob is the test, and it is worth running because the two outcomes say opposite things:
//
//   the picture stays correct -> the single rasterisation I measured is NOT the whole story; the second view's
//                               graph draws the cascades too, the duplicate is real, and reuse is available
//                               together with whatever the pass costs in a dense scene
//   the shadows break again   -> the pass really is single and lives in MAIN's graph, so dropping MAIN's draws
//                               removes the only rasterisation there is. That also explains every earlier
//                               failure without appealing to barriers, ordering or resource identity
//
// Either way it is one launch for an answer that three earlier rounds argued about. Failure mode, written down
// first: sun shadows wrong or missing in one eye, image geometry untouched. Set to 0 to revert.
//
// LIVE, as xr_cascade_save_main in vrport.ini -- 1 keeps the fix, 0 lets MAIN clear the shared
// atlas again (vanilla behaviour, duplicate rasterisation and all). It reads on the next poll,
// so the revert above no longer needs a debugger.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CascadeSaveMain = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCascadeDrawsSaved = 0;

// True when this node's work context belongs to the second view. Separate from the probe paths because the
// skip must not depend on a diagnostic flag being on.
static bool casc_ctx_is_vrcam(void* a2, bool* known) {
    *known = false;
    if (!a2) return false;
    __try {
        const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(a2) + 0x18);
        if (!ctx) return false;
        *known = true;
        return *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
    } __except (EXCEPTION_EXECUTE_HANDLER) { *known = false; return false; }
}

// True when this cascade node belongs to MAIN and the given knob is armed. A view whose context cannot be
// read is left alone: guessing "not the second view, therefore MAIN" would silently cut the node for
// anything unidentified.
static bool casc_is_main(void* a2) {
    bool known = false;
    const bool vrcam = casc_ctx_is_vrcam(a2, &known);
    return known && !vrcam;
}

static bool casc_skip_for_main(void* a2) {
    if (!CyberpunkVR_CascadeSkipMain) return false;
    bool known = false;
    const bool vrcam = casc_ctx_is_vrcam(a2, &known);
    if (!known || vrcam) return false;
    InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCascadeMainSkips));
    return true;
}

// ---- FINDING THE COMMAND LIST THE ENGINE HOLDS ---------------------------------------------------
//
// The port's PIX markers are emitted 198038 times a run and appear in a RenderDoc capture exactly zero times.
// Both numbers are measured, and together they say where we sit: our command-list vtable hook is on the REAL
// D3D12Core list (distinctVtables=1, found through our own creation path), while the game records into
// RenderDoc's WRAPPER. Anything we call on the real list is below the capture layer and is recorded by nobody.
//
// So the marker has to go to the object the ENGINE holds, and this probe finds it by the one signature that
// cannot be mistaken: a pointer inside the node's work context whose VTABLE lies inside renderdoc.dll. Nothing
// else in this process has that shape. With a capture layer absent it reports d3d12core vtables instead, which
// is the same slot and is how the offset is confirmed without a capture running.
//
// Every dereference is guarded by VirtualQuery rather than by __try, deliberately: this project has already
// established that the engine's own VEH swallows SEH first, so a bad read crashes the game even inside a
// __try. Asking the OS whether a page is committed and readable is the only guard that actually holds.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CmdListHunt = 1;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugCmdListOffset = 0xFFFFFFFFu;

namespace {
bool cvr_readable(const void* p, size_t n) {
    if (!p) return false;
    MEMORY_BASIC_INFORMATION mbi = {};
    if (!VirtualQuery(p, &mbi, sizeof(mbi))) return false;
    if (mbi.State != MEM_COMMIT) return false;
    if (mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD)) return false;
    const uintptr_t end = reinterpret_cast<uintptr_t>(mbi.BaseAddress) + mbi.RegionSize;
    return reinterpret_cast<uintptr_t>(p) + n <= end;
}

// The module a code address belongs to, base name only, or nullptr.
const char* cvr_module_of(const void* addr) {
    HMODULE mod = nullptr;
    if (!GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                            GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                            reinterpret_cast<LPCSTR>(addr), &mod) || !mod)
        return nullptr;
    static thread_local char path[MAX_PATH];
    if (!GetModuleFileNameA(mod, path, MAX_PATH)) return nullptr;
    const char* slash = strrchr(path, '\\');
    return slash ? slash + 1 : path;
}

// Walk a context object for candidate COM pointers and report the ones whose vtable is in a graphics or
// capture module. Bounded, read-only, and throttled: this is a hunt, not a per-frame probe.
void cmdlist_hunt(const void* obj, const char* what, uint32_t bytes) {
    if (!CyberpunkVR_CmdListHunt || !obj) return;
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 10000) return;
    s_last = now;

    char line[900];
    int used = 0, hits = 0;
    line[0] = 0;
    for (uint32_t off = 0; off + 8 <= bytes; off += 8) {
        const void* const* slot = reinterpret_cast<const void* const*>(
            reinterpret_cast<const uint8_t*>(obj) + off);
        if (!cvr_readable(slot, 8)) continue;
        const void* cand = *slot;
        if (!cvr_readable(cand, 8)) continue;
        const void* vt = *reinterpret_cast<const void* const*>(cand);
        if (!cvr_readable(vt, 8)) continue;
        const void* first = *reinterpret_cast<const void* const*>(vt);
        if (!cvr_readable(first, 8)) continue;
        const char* mod = cvr_module_of(first);
        if (!mod) continue;
        const bool interesting = _stricmp(mod, "renderdoc.dll") == 0 ||
                                 _strnicmp(mod, "d3d12", 5) == 0 ||
                                 _stricmp(mod, "nvwgf2umx.dll") == 0;
        if (!interesting) continue;
        ++hits;
        if (_stricmp(mod, "renderdoc.dll") == 0 && CyberpunkVR_DebugCmdListOffset == 0xFFFFFFFFu)
            CyberpunkVR_DebugCmdListOffset = off;
        if (used < 780)
            used += snprintf(line + used, sizeof(line) - used, "+0x%X->%s  ", off, mod);
    }
    log("[cmdhunt] %s: %d candidate COM pointers with a graphics/capture vtable | %s",
        what, hits, hits ? line : "(none)");
}
}  // namespace

using CascadeNodeFn = int64_t(__fastcall*)(void*, void*);
static CascadeNodeFn g_orig_cascade_node = nullptr;
static CascadeNodeFn g_orig_cascade_clear = nullptr;

// The clear NODE, cut only by the measured-harmful SkipMain. SaveMain deliberately lets it run: the capture
// shows this node making the atlas's NON_PIXEL_SHADER_RESOURCE -> DEPTH_WRITE transition, and cutting the
// node takes that transition with it -- which is what my first SaveMain did, and why it flickered exactly
// like the node cut. SaveMain now drops only the ClearDepthStencilView command, in hk_ClearDepthStencilView.
static int64_t __fastcall Detour_CascadeClear(void* a1, void* a2) {
    if (casc_skip_for_main(a2)) return 1;      // 1 = the port's standing "node handled" return, see NodeCut
    return g_orig_cascade_clear(a1, a2);
}
CVR_DETOUR("[cascade] clear sub_141D59B40", CASCADE_CLEAR_NODE_RVA, Detour_CascadeClear, g_orig_cascade_clear)

static int64_t __fastcall Detour_CascadeNode(void* a1, void* a2) {
    if (casc_skip_for_main(a2)) return 1;
    if (!a1) return g_orig_cascade_node(a1, a2);
    // The index is read UNCONDITIONALLY now, not behind CascadeIdxProbe. A FIX depends on it: the cascade
    // camera lend in Grading.cpp keys its snapshot by this index, and keying that 848-byte block by
    // (node, size) alone would cross cascade 0 with cascade 1 -- the trap this file already warns about for
    // exactly that row of the census. A fix that quietly stops working when a diagnostic flag is off is the
    // kind of thing this project has been bitten by more than once.
    if (CyberpunkVR_CmdListHunt && a2) {
        cmdlist_hunt(a2, "node work object", 0x400);
        uintptr_t ctx = 0;
        if (cvr_readable(reinterpret_cast<uint8_t*>(a2) + 0x18, 8))
            ctx = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(a2) + 0x18);
        if (ctx) cmdlist_hunt(reinterpret_cast<const void*>(ctx), "render context", 0x2000);
    }
    const int32_t prev = t_cascade_idx;
    int32_t idx = -1;
    __try {
        idx = *reinterpret_cast<int32_t*>(reinterpret_cast<uint8_t*>(a1) + 24);
    } __except (EXCEPTION_EXECUTE_HANDLER) { idx = -1; }
    t_cascade_idx = idx;

    // Count the dispatch itself, per view and per cascade -- the cheapest possible measurement and the one
    // that says whether the second view renders every cascade at all. ANSWERED, so default 0: in gameplay
    // casc0 M=2410 V=2410 and casc1 M=2410 V=2410, with cascades 2 and 3 rendered by NEITHER view, and the
    // per-interval deltas identical. Both eyes render every cascade there is.
    if (CyberpunkVR_CascCountProbe && idx >= 0 && idx < 8 && a2) {
        __try {
            const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
                reinterpret_cast<uint8_t*>(a2) + 0x18);
            const bool vrcam = ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
            ++g_casc_dispatch[vrcam ? 1 : 0][idx];
        } __except (EXCEPTION_EXECUTE_HANDLER) { }
        casc_count_report();
    }

    // The cascade placement: snapshot it from the view that runs FIRST and give it to the one that runs
    // second, so both eyes rasterise on one texel grid within the same frame.
    if (CyberpunkVR_CascRecLend && idx >= 0 && idx < 8 && a2) {
        __try {
            const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
                reinterpret_cast<uint8_t*>(a2) + 0x18);
            const uintptr_t mgr = ctx ? *reinterpret_cast<uintptr_t*>(ctx + 0x1E10) : 0;
            const bool vrcam = ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
            if (mgr) {
                uint8_t* rec = reinterpret_cast<uint8_t*>(mgr + kCascRecBase + kCascRecStride * idx);
                if (vrcam) {
                    memcpy(g_casc_rec[idx], rec, kCascRecScan);
                    g_casc_rec_have[idx] = true;
                } else if (g_casc_rec_have[idx]) {
                    casc_rec_report(idx, rec, g_casc_rec[idx]);
                    if (CyberpunkVR_CascFitProbe) cascfit_note_pre(rec, idx);
                    // THE WHOLE FIT, not two field groups. The first attempt copied only the extent and the
                    // centre and left everything else -- including the transform the pass actually builds its
                    // constants from, which sits elsewhere in this record -- so MAIN kept rasterising and
                    // sampling with its own fitting and nothing changed. Copying two fields out of a fit is
                    // not lending the fit.
                    //
                    // Everything from +0x10 on is safe to copy because this record is ONE SHARED object that
                    // each view rewrites before its own pass; there is no per-view half of it to protect. The
                    // first sixteen bytes are left alone: they change every frame (serials, measured across
                    // three dumps) and copying them buys nothing.
                    memcpy(rec + 16, g_casc_rec[idx] + 16, kCascRecScan - 16);
                    InterlockedIncrement64(
                        reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCascRecLends));
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { }
    }

    // How far apart the two views fit this cascade, in metres, read AFTER the lend on purpose. With the lend
    // off it measures the engine's own separation (9-13 cm on cascade 0, 21-36 cm on cascade 1 while turning);
    // with the lend on it must read zero, and that is the only way to tell a lend that landed from one that
    // wrote the wrong fields. Placed here, not before, precisely because the earlier version of this pairing
    // would have reported the pre-patch difference and looked like a failure either way.
    if (CyberpunkVR_CascFitProbe && idx >= 0 && idx < 8 && a2) {
        __try {
            const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
                reinterpret_cast<uint8_t*>(a2) + 0x18);
            const uintptr_t mgr = ctx ? *reinterpret_cast<uintptr_t*>(ctx + 0x1E10) : 0;
            const bool vrcam = ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
            if (mgr)
                cascfit_note(reinterpret_cast<const uint8_t*>(mgr + kCascRecBase +
                                                              kCascRecStride * idx), idx, vrcam);
        } __except (EXCEPTION_EXECUTE_HANDLER) { }
        cascfit_report();
    }

    if (CyberpunkVR_CascadeBiasLend && idx >= 0 && idx < 8 && a2 && g_exe_base) {
        if (!g_casc_viewdata)
            g_casc_viewdata = reinterpret_cast<CascViewDataFn>(g_exe_base + 0x1ED930);
        __try {
            uint8_t* vd = reinterpret_cast<uint8_t*>(g_casc_viewdata(a2));
            const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
                reinterpret_cast<uint8_t*>(a2) + 0x18);
            const bool vrcam = ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
            if (vd) {
                float* slot = reinterpret_cast<float*>(vd + kCascBiasOffset + 16 * idx);
                if (!vrcam) {
                    g_casc_bias_main[idx] = *slot;
                    g_casc_bias_have[idx] = true;
                } else if (g_casc_bias_have[idx]) {
                    casc_bias_report(idx, g_casc_bias_main[idx], *slot);
                    if (*slot != g_casc_bias_main[idx]) {
                        *slot = g_casc_bias_main[idx];
                        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
                            &CyberpunkVR_DebugCascBiasLends));
                    }
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { }
    }

    const int64_t r = g_orig_cascade_node(a1, a2);
    t_cascade_idx = prev;      // restored, or a nested node would leave the wrong index behind
    return r;
}

// ---- registered where they are defined ---------------------------------------------------------
CVR_DETOUR("[cascade] shadow-cascade node sub_140153844", CASCADE_NODE_RVA, Detour_CascadeNode, g_orig_cascade_node)
CVR_DETOUR("[foliage] speedtree wind sub_140CC4DF4", SPEEDTREE_WIND_RVA, Detour_SpeedTreeWind, g_orig_st_wind)
CVR_DETOUR("[foliage] wind-impulse node sub_1406EAEDC", WIND_IMPULSE_NODE_RVA, Detour_WindImpulseNode, g_orig_wind_node)
CVR_DETOUR("[clouds] cloud-CB sub_140784654", CLOUD_CB_FILL_RVA, Detour_CloudCbFill, g_orig_cloud_cb)
CVR_DETOUR("[distant] distant-prepare sub_140374AD8", DISTANT_PREPARE_RVA, Detour_DistantPrepare, g_orig_distant_prepare)
CVR_DETOUR("[distant] distant-render sub_140373998", DISTANT_RENDER_RVA, Detour_DistantRender, g_orig_distant_render)
CVR_DETOUR("[gi] GI-node sub_14077E664", GI_NODE_RVA, Detour_GiNode, g_orig_gi_node)
CVR_DETOUR("[localshadow] local-shadow sub_140AD5770", LOCAL_SHADOW_RVA, Detour_LocalShadowMaps, g_orig_local_shadow)
CVR_DETOUR("[probes] reflection-probe atlas sub_14077E610", REFLECTION_PROBES_RVA, Detour_ReflectionProbes, g_orig_reflection_probes)
CVR_DETOUR("[viewrect] sky-scattering node sub_1407818B0", SKY_SCATTERING_RVA, Detour_SkyScattering, g_orig_sky_scatter)

// ================================================================================================
// MORE OF THE SAME FAMILY, moved out of the monolith: the amortised sky, the GI reuse mode, and the
// lending of MAIN's draw-block list to the light-volume pass.
//
// All three are the rule at the top of this file applied again -- an expensive producer runs once and
// the second view reads its result. The sky is the one that fights back: it is amortised across frames,
// so the second view asking for it mid-amortisation is not free, and the mode knob exists because the
// right answer depended on measurement rather than principle.
// ================================================================================================

// ---- the amortised sky, and the second view fighting MAIN for it ---------------------------
//
// PROVEN, not inferred. sub_1407818F8 (the body of CRenderNode_RenderSkyScattering) builds the
// sky in SIX INSTALMENTS into a 32-byte record picked by `32 * *(BYTE*)(view + 0x16E0)`:
//
//     v15 = *(BYTE*)(rec + 80);                  // the shared slot cursor
//     do { v16 = 1 << v15++; v17 = v14 & v16; } while (!v17);
//     *(BYTE*)(rec + 80) = v15;                  // ADVANCE IT
//     sub_140783384(a2, v17, ...);               // fill that slot FROM THIS VIEW
//     if (*(BYTE*)(rec + 80) >= 6) { publish; slot = 0; InterlockedExchange(rec+72, 0); }
//
// and the live probe says both views index the SAME record:
//     [sky] sky-record index view+0x16E0 -- MAIN 0  VRCAM 0     AA mode -- MAIN 0  VRCAM 0
//
// AA mode 0 on both means `!v4` holds for both, so both views pass the gate and both advance one
// cursor. The published sky is therefore assembled from alternating instalments of two different
// cameras -- MAIN fills slot 0, VRCAM slot 1, MAIN slot 2, and so on -- and it reaches six in
// half the frames, so it republishes twice as often, each time half-wrong. Which view lands on
// which slot drifts, so the result changes shape from frame to frame. At night that LUT is what
// carries the stars and the horizon, which is exactly the reported "no stars, the sky is not
// like that". The enabled-slot mask itself is per view too (`v14 = v28 ? -1 : -17`, and v28 is
// sub_140B2CB98(a2, ...)), so the two are not even filling the same set.
//
// The remedy is the one distant shadows and local shadow maps both needed, for the same reason:
// let ONE view drive the shared structure and have the other consume the published result. The
// sky LUT is a function of sun direction and altitude, not of view direction, so MAIN's answer
// is correct for an eye 6.5 cm away -- the same argument that makes cascade and GI reuse sound.
//
// 1 = skip the sky build for the second view (it samples MAIN's published sky).
// 0 = both views build it, i.e. the shipped behaviour, for A/B.
using SkyWorkFn = void(__fastcall*)(void*, void*);
SkyWorkFn g_orig_sky_work = nullptr;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_SkyReuseMode = 0;   // A/B: does the second view get its cloud state back?
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugSkySkipHits = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugSkyMainHits = 0;

// Sky, clouds and shadow reuse moved to src/Stereo/ViewReuse.cpp.

// --- Reflection-probe reuse for VRCAM ------------------------------------
// CRenderNode_ReflectionProbes sub_14077E610 updates the SHARED reflection-probe manager
// v4 = *(qword_143427C00 + 200) (renderer is a GLOBAL singleton, NOT per-view ctx) via
// sub_14077E030 + sub_14077FCDC. Manager is structurally view-INDEPENDENT => reuse-safe by
// construction. Probes are world-space env captures; BINDING is a separate node
// (CRenderNode_BindEnvProbes, runs for vrcam) so skipping the UPDATE for vrcam => vrcam binds
// MAIN's updated probe atlas = reuse. VRCAM does 4.3x main's probe work (redundant rebuild).
// Same ABI: a2=rdx, ctx = *(a2+0x18), key @ ctx+0x28.

// --- GI (diffuse indirect) reuse for VRCAM -------------------------------
// CRenderNode_GlobalIllumination sub_14077E664 flow (from live disasm):
//   result = sub_1401E4B60(*(a2+0x18)+0x14);            // early-out check
//   if (result) return result;                          // (jnz end)
//   renderer = *qword_143427C00; mgr = *(renderer+0xB8);
//   if (mgr) { dirty-refresh; if (bit31) sub_14077F758(mgr,a2,&camVec,..); }  // UPDATE builds
//                                                                             // SHARED 122
//   applyMgr = *(renderer+0xC0); if (applyMgr) return sub_14077E74C(applyMgr, a2); // APPLY
// Both views build the SHARED committed GI cache Resource_122 (10243 R11G11B10) -> redundant.
// Goal: vrcam should REUSE main's 122 (skip its build) but STILL apply, AND keep GIVolumes.
// Two dead ends: (1) hooking update fn sub_14077F758 -> CRASH (apply got garbage a2, minidump:
// sub_14077E74C+0x24). (2) clearing bit31 (VrcamFlagMode=3) skipped update cleanly BUT bit31
// also gates RenderGIVolumes (sub_140B779DC = interior light) -> interior went dark.
// SOLUTION: hook the GI NODE and, for vrcam, REPLICATE its flow minus the update+dirty-refresh
// (both write the shared mgr/122; main maintains them). Full control of a2 (no corruption),
// ctx bit31 untouched (GIVolumes, a SEPARATE node, still runs), apply reuses main's 122.
// RVAs verified from live disasm @ base 0x7FF6EF660000.

// DEFAULT 0 since 2026-07-28. A night capture (Objects/EventList_LATESTVR) shows MAIN running
// a lighting stage VRCAM does not: PSO 1030 (5424 + 4166 groups), 671 (1943 + 1636), 1316
// (16^3) and 1336 x16 over 3D grids, writing the shared GI cache Resource_122 (1024x1024x3
// R11G11B10), the 64^3 clipmap volumes Resource_3164/3166 and a 112 MB probe buffer. That is
// this hook skipping the GI build for VRCAM, and the audit agrees: GlobalIllumination is
// MAIN 0.1022 ms vs VRCAM 0.0289 ms. The reuse was measured as harmless earlier -- but in
// DAYLIGHT, where the stage is inert; at night it is what lights the street lamps.
// BACK TO 1 (2026-07-31). The 0 above was set on 2026-07-28 for the night street lamps, and
// that theory is recorded as DISPROVEN in the same hunt: all three reuse knobs off changed
// nothing for the lamps, which were finally fixed by RenderMask/DistantLights instead. What
// the 0 did keep doing is let VRCAM rebuild the SHARED GI every frame from its own frustum --
// cache Resource_122, the 64^3 clipmaps 3164/3166, the 112 MB probe buffer -- and the note at
// GI_FEATURE_BIT says exactly what that costs: "main GI (ambient light/shadows) flicker".
// It is also the ONLY reuse knob that differs from the known-good testbed snapshot of 25 Jul.
// AT 0 FOR AN INTERIOR TEST 2026-08-18, and the standing warning above is exactly why this is a test and
// not a change. The pipeline sweep says the two views do not merely run this node a different number of
// times, they run DIFFERENT compute shaders inside it: VRCAM 3 actions (cs 1911, 2574, 2058) against
// MAIN 28 (cs 2058, 2058, 1174). So reuse here is not "the same apply over reused data", it is another
// branch -- and indoors the indirect light IS the lighting, which is the reported symptom: ceiling too
// shiny, glasses lit cold instead of warm like the room, blown highlights, haze wrong. The flicker
// verdict that pinned this knob at 1 was measured OUTDOORS; indoors it was never tried.
//
// EXPECTED COST AT 0, from the record above: position-dependent shadow flicker, two shadow sets
// alternating in one spot and clean in others, because the second view rebuilds a shared structure from
// its own frustum. If that appears it is the price of the test, not a new defect. Back to 1 either way
// unless the interior mismatch follows this knob.
// AND THE INTERIOR A/B CAME BACK NEGATIVE, TWICE OVER (user, 2026-08-18). At 0 the ceiling and the rest of
// the interior artefacts were UNCHANGED, so GI reuse is not the cause; and the one thing that did change
// got WORSE -- the glasses went from bluish to outright white, i.e. the second view building its own GI is
// further from MAIN than reusing MAIN's. The predicted price also arrived on cue: shadows started
// flickering, which is the position-dependent flicker this file already records against this knob and is
// what confirms the knob really took effect rather than sitting inert.
//
// So the different compute shaders in this node (VRCAM cs 1911/2574/2058 against MAIN cs 2058/2058/1174)
// are a real pipeline difference and still NOT the interior defect. Back at 1, and do not spend another
// round here.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_GiReuseMode = 1;   // 0=vrcam builds its own GI, 1=reuse main's (A/B)
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugGiSkipHits = 0;

// GI reuse moved to src/Stereo/ViewReuse.cpp.

// --- Shared culling for VRCAM (reuse main's visibility) -------------------
// CRenderNode_DoCulling (sub_140B2BEFC, gate owner-bit a2+0x30&2) snapshots a 0x48-byte
// per-view frustum descriptor (sub_1401EC7EC) and submits cull JOBS -- "DoCull_MainScene"
// (node bit1 -> sub_1406246E8) and "DoCull_Cascades" (bit50 -> sub_140C43954) -- to the
// global multi-frustum visibility system (qword_143438980, worker-pool + semaphore). The
// cull work writes visibility into the SCENE MANAGER at *(view_ctx+0x1E10), which is
// VERIFIED shared (identical ptr) between vrcam & main; occluders are gathered WORLD-SPACE.
// STEP 1 (this): skip vrcam's DoCulling entirely. If the shared manager makes vrcam's render
// reuse main's visible set -> near-free reuse (then expand main's frustum a touch to cover
// vrcam conservatively). If vrcam goes empty -> visible set is per-view-indexed -> redirect
// explicitly. Toggle LIVE via x64dbg (default OFF); watch fps + vrcam image. a2=view arg,
// ctx=*(a2+0x18), key@ctx+0x28 (same ABI as the other reuse skips).
using DoCullingFn = char(__fastcall*)(void*, void*, void*);
DoCullingFn g_orig_doculling = nullptr;

// ---- lend MAIN's draw-block list to the light-volume pass -----------------------------------
// The indirect census settles what was missing: RenderLightBuffers (0x77D308) issues an indirect
// DRAW with the signature that AutoSpawnOnTerrain and RenderShadowCascade also use, and VRCAM
// issues it ZERO times -- in any node. It is not the capability gate (granted, census unchanged).
// The common cause is the one this project already recorded while chasing the HUD: the RTT view
// never collects draw blocks, so there is nothing for an indirect draw to consume. Light volumes
// are drawn geometry, which is why the sun and sky are unaffected and only local lights vanish.
//
// The fix borrows the mechanism already proven at DrawComposition (CullReuseMode 5): hand the
// view MAIN's block list for the duration of ONE call and put its own pointer back in a
// __finally. Nothing is fabricated -- this is MAIN's live pointer, and it is never left in place,
// which is what separates this from the two crashes that came of faking viewData+0x168.
using LightBufFn = __int64(__fastcall*)(void*, void*);
static LightBufFn g_orig_lightbuffers = nullptr;
// DEFAULT 0 -- this CRASHES: EXCEPTION_ACCESS_VIOLATION reading 0x10, i.e. a null deref one
// level inside the borrowed list. Lending MAIN's real pointer scoped to a single call is
// safe at DrawComposition (CullReuseMode 5) but NOT here: RenderLightBuffers walks further
// into the block list and reaches per-view resources the RTT view does not own. That makes
// three separate crashes from writing viewData+0x168 -- treat the field as unusable and fix
// the RTT view's own block collection instead.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_LightBlockLend = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLightBlockLends = 0;

using ViewDataGetterFn = __int64(__fastcall*)(void*);
static ViewDataGetterFn g_viewdata_get = nullptr;   // lazy init to sub_1401ED930
static __int64 __fastcall Detour_LightBuffers(void* a1, void* a2) {
    if (!CyberpunkVR_LightBlockLend || !a2 || !t_vrcam_node_active || !g_main_block_v5)
        return g_orig_lightbuffers(a1, a2);
    if (!g_viewdata_get && g_exe_base)
        g_viewdata_get = reinterpret_cast<ViewDataGetterFn>(g_exe_base + 0x1ED930);
    if (!g_viewdata_get) return g_orig_lightbuffers(a1, a2);
    void** slot = nullptr;
    void* saved = nullptr;
    __try {
        uint8_t* viewData = reinterpret_cast<uint8_t*>(g_viewdata_get(a2));
        if (viewData) {
            slot = reinterpret_cast<void**>(viewData + 0x168);
            saved = *slot;
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) { slot = nullptr; }
    // Only lend where the view has nothing of its own; never displace a real list.
    if (!slot || saved) return g_orig_lightbuffers(a1, a2);
    __int64 r = 0;
    __try {
        *slot = g_main_block_v5;
        InterlockedIncrement64(
            reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugLightBlockLends));
        r = g_orig_lightbuffers(a1, a2);
    } __finally {
        *slot = saved;
    }
    return r;
}

// ---- THE COMPOSITION STATE, LENT FOR ONE NODE CALL AND NO MORE --------------------------------
//
// viewData+0x168 is the gate on four nodes the engine dispatches for MAIN and never for this view:
// StartRender, DeclareCommonResourceAllocs_FinalOnly, CompositionPostProcess and DrawComposition.
// Writing the field opens ALL of them, and that is what the previous attempt did -- installed on
// every dispatch of our view and left in place. It ran the stage, and it crashed twice in one
// afternoon, both times inside DrawComposition and never inside CompositionPostProcess:
//
//   exe+0x3C6F5C   *(a1 + 0x70) with a1 == 0 -- the null is *(record + 0x28) of a type-3
//                  composition element, loaded at exe+0x1ECE2A and passed down three frames untested
//   exe+0xA03322   mov eax,[rcx+0x18] with rcx = 0x34E0304032662E56 -- garbage read out of a
//                  56-byte-stride array of composition elements
//
// Both with the view identified in the same frame by the camera-name hash at view+0x28. The reason
// is not the gate: DrawComposition walks a LIST of composition elements assembled for another view
// and runs off its end. Giving that node the stage as well needs the second view to own a
// composition set, which is a separate piece of engineering and not a knob.
//
// So the lend is scoped two ways instead of none. By TIME: installed before the original call and
// removed on return, so nothing outside the wrapped dispatch ever sees the field set. By NODE: one
// bit per node, because which nodes actually need it is NOT settled --
//
//     bit 0   CompositionPostProcess (0x1F8928)          the target, try this alone first
//     bit 1   DeclareCommonResourceAllocs_FinalOnly      only if bit 0 alone is not enough
//
// DrawComposition is not offered a bit at all. It is the one that crashes, and no amount of scoping
// changes that its element list belongs to another view.
//
// Why bit 1 is a bit and not a given. The record of the first lend attempt says two different things
// about the same crash -- an access violation reading 0x68 of a null object -- and only one of them
// can be true. It says composition against undeclared resources caused it, which is what would make
// the declaration pass necessary; and it also says those four crashes correlated with the feature's
// key EVEN IN RUNS WHERE IT CAPTURED AND INSTALLED NOTHING, i.e. that touching MAIN's composition
// dispatch was what broke, which would make the declaration pass irrelevant. The second reading is
// the better evidence -- a crash in runs that changed nothing cannot be caused by what the change
// would have done -- so the declaration pass is a hypothesis here, not a requirement, and it gets a
// bit to be tested with rather than a place in the default.
//
// The source is the half of the previous attempt that was right, and every clause of it is a
// measurement. The pair is read FRESH from the set on every use, because the set object is stable
// while the pair inside it is swapped per frame -- {1B50C817D90,1B50411F9C0} then
// {1B508752630,1B528DB7240} in consecutive frames. The engine's own assign sub_140201A68 is never
// called: it does InterlockedExchange(set+8, 0), a one-shot per frame, and a second caller pushes
// MAIN onto the other branch on alternate frames -- that was MAIN's judder and the flickering HUD.
// viewData+0xF88 is never written: CRenderNode_Present reads the same field and would enlist this
// view in presenting to the desktop window. And the refcount is written as 0 on purpose, because
// the engine's release reads `if (!refcount || InterlockedExchangeAdd(refcount,-1) != 1) return 0;`
// -- so every release of our copy is a no-op, and MAIN's object can neither be freed nor have its
// count moved.
//
// WHICH NODES, MEASURED -- and a wrong conclusion of mine withdrawn along the way.
//
// The first scoped attempt gave CompositionPostProcess the field and nothing else, and the node
// died in the frame graph's resource lookup, sub_1406E8884, with the resolved index ZERO:
//
//     mov   eax, [rdx]          ; the resolved resource index for the key
//     dec   eax                 ; 0 - 1 == 0xFFFFFFFF, and rax in the dump is exactly that
//     imul  r9, rax, 0B0h       ; r9 = table base + 0xFFFFFFFF*176
//     movzx r8d, [r9+2F22Ch]    ; fault, and 0x29D1519F17C minus r9 is exactly 0x2F22C
//
// I read that as proof that the whole route is impossible -- that frame-graph resources are fixed
// when the graph is built, so opening the gate at dispatch time can never work. That is WRONG, and
// the port's own record refutes it: the unscoped version of this lend made the stage run, with
// counters, `CompositionPostProcess M=69896 V=2536`, `192x192 M=17474 V=634` -- all zero for the
// whole session before it. If the resources were fixed at build time, no version could have run.
// The only difference between the two is HOW MANY NODES see the field, so the resource is created
// during the frame by one of the other readers, and the scoped version simply starved it.
//
// So the readers were measured instead of assumed: of the 68 functions that call the viewData
// accessor sub_1401ED930, exactly five touch +0x168.
//
//     0x1F8928  CompositionPostProcess    bit 0   the target
//     0xC66188  render-target declarations bit 1  names RT_viewDepthTexture[%d] and
//                                                 RT_localShadowTexture[%d] -- the likely creator
//                                                 of what the composition node then looks up
//     0x1EE760  DrawHUD                   bit 2
//     0x770268  unnamed, vtable-only      bit 3
//     0x20A264  DrawComposition           NO BIT -- this is the one that crashes, twice measured,
//                                         because it walks an element list built for another view
//
// And DeclareCommonResourceAllocs_FinalOnly (0x1EE4A0) is NOT a reader at all, so the bit I gave
// it in the first attempt could never have done anything. That is what came of taking a note as a
// measurement.
//
// Start at 3 -- the composition node and the declarations that feed it. Ships off.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CompLendScoped = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCompScopedSet = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCompScopedLends = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCompScopedSkips = 0;

using CompAssignFn = __int64 (__fastcall*)(__int64, __int64);
static CompAssignFn g_orig_comp_assign = nullptr;
static std::atomic<uintptr_t> g_comp_set{0};

// Learns the set pointer and nothing else. Gated on branch A's precondition read AT ENTRY, before
// the original consumes the one-shot flag, so the pointer learned is the one that owns the handle.
static __int64 __fastcall Detour_CompAssign(__int64 setObj, __int64 src) {
    if (setObj > 0x10000) {
        uint32_t gate[2] = {0, 0};
        if (cloud_cb_raw_copy(gate, reinterpret_cast<const uint8_t*>(setObj) + 8, 8) &&
                gate[0] && gate[1]) {
            g_comp_set.store(static_cast<uintptr_t>(setObj), std::memory_order_release);
            InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCompScopedSet));
        }
    }
    return g_orig_comp_assign(setObj, src);
}
CVR_DETOUR("[complend] composition state assign sub_140201A68", COMP_ASSIGN_RVA,
           Detour_CompAssign, g_orig_comp_assign)

// ==== RESTORED FROM THE 0.1.6 RENDER (7dd29c36), which 747a9983 took back to 0.1.5 ========
// The set pointer, the assign detour and its globals already survive further down this file with the
// SCOPED variant; only the unscoped switch and the write itself were taken out, so only those come
// back, and both read the same g_comp_set.

//
// Brought back for the braindance scanner outline. In a braindance the outline is not written by
// RenderVisionElements at all -- measured with the vismap live in that scene, the node appears for
// NEITHER view -- it is produced inside DrawComposition, which the engine runs for MAIN only. So
// the second eye needs its composition to RUN, and this is what makes it run.

// --- LENDING THE COMPOSITION STATE, WITHOUT DISTURBING MAIN'S ----------------------------------
//
// CompositionPostProcess is dispatched for the second view and bails on
//     call <accessor>; mov rdx,[viewData+0x168]; test edx,edx; jz <skip the whole stage>
// so that one refcounted handle pair is the entire gate. Running the engine's own provider to fill it
// DOES work -- the stage's UAV passes went from V=0 to V=3264 -- but it wrecked MAIN, and all three
// reasons were then read out of the engine's own code instead of guessed:
//
//  1. The provider ends in sub_140201A68, which does `InterlockedExchange(set+8, 0)` and takes the
//     handle at *(set+176) only while that flag is up, *(set+184) otherwise. The flag is ONE-SHOT per
//     frame, so calling it a second time for our view left MAIN on the other branch on alternate
//     frames -- exactly the "MAIN juddering as if the IPD moved" and the flickering HUD.
//  2. Feeding the provider requires viewData+0xF88 (its output object), and CRenderNode_Present reads
//     the SAME field: `if (*(vd+0xF88) && *(vd+0x150))` it drives the renderer and calls InvalidateRect
//     on the window's HWND. A non-null +0xF88 enlists our view in presenting to the desktop window --
//     the black frames. So that field must stay zero, and this route never writes it.
//  3. The state object is PER FRAME: breaking on the assign in two consecutive frames read the pair as
//     {1B50C817D90, 1B50411F9C0} and then {1B508752630, 1B528DB7240}, while the set object was
//     1B52142C3A0 both times. Caching the pair would be the same use-after-free that produced this
//     evening's light/dark flicker, so it is read FRESH from the stable set on every use.
//
// What this does instead: read the pair the engine would have assigned, straight out of the set, and
// write it into our own block as {value, NULL refcount}. The null refcount is deliberate and safe by
// measurement -- the engine's release helper is `if (!refcount || --refcount != 1) return 0;`, so every
// release of our copy is a no-op: we can neither free MAIN's object nor perturb its count, and MAIN's
// own strong reference keeps it alive through the frame we borrow it in.
//
// It is installed on EVERY dispatch of our view, not only the composition one, because
// CRenderNode_DeclareCommonResourceAllocs_FinalOnly gates part of its declarations on the same field
// and runs earlier in the frame. A composition node running against undeclared resources is what read
// 0x68 of a null object in the first lend attempt.


extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CompLendSet = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCompLendWrites = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCompLendSkips = 0;

static void vd_lend_composition(uintptr_t vd) {
    if (!vd || !CyberpunkVR_CompLendSet) return;
    const uintptr_t set = g_comp_set.load(std::memory_order_acquire);
    if (!set) return;
    uint64_t holder = 0;                                   // *(set + 176): branch A's holder
    if (!cloud_cb_raw_copy(&holder, reinterpret_cast<const uint8_t*>(set) + 176, 8) || !holder) {
        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCompLendSkips));
        return;
    }
    uint64_t pair[2] = {0, 0};                             // {value, refcount} at holder + 600
    if (!cloud_cb_raw_copy(pair, reinterpret_cast<const uint8_t*>(holder) + 600, 16) || !pair[0]) {
        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCompLendSkips));
        return;
    }
    // The node loads the pass size out of this object at +0x50/+0x54 and both eyes render at 3072, so
    // requiring a full-size object is the same guard, for the same reason, as the viewData capture: a
    // recycled or half-built one does not carry it.
    uint32_t wh[2] = {0, 0};
    if (!cloud_cb_raw_copy(wh, reinterpret_cast<const uint8_t*>(pair[0]) + 0x50, 8) ||
            wh[0] < 2048 || wh[1] < 2048) {
        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCompLendSkips));
        return;
    }
    uint64_t current[2] = {0, 0};
    if (!cloud_cb_raw_copy(current, reinterpret_cast<const uint8_t*>(vd) + 0x168, 16)) return;
    if (current[0] == pair[0] && current[1] == 0) return;   // already lent, and this frame's object
    const uint64_t lent[2] = {pair[0], 0};                  // the value, and NO refcount, on purpose
    if (cloud_cb_raw_copy(reinterpret_cast<uint8_t*>(vd) + 0x168, lent, 16))
        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCompLendWrites));
}


// The pair the engine would have assigned, read out of the stable set every single time.
static bool comp_lend_fetch(uint64_t out[2]) {
    const uintptr_t set = g_comp_set.load(std::memory_order_acquire);
    if (!set) return false;
    uint64_t holder = 0;                                    // *(set + 176): branch A's holder
    if (!cloud_cb_raw_copy(&holder, reinterpret_cast<const uint8_t*>(set) + 176, 8) || !holder)
        return false;
    uint64_t pair[2] = {0, 0};                              // {value, refcount} at holder + 600
    if (!cloud_cb_raw_copy(pair, reinterpret_cast<const uint8_t*>(holder) + 600, 16) || !pair[0])
        return false;
    // The node loads the pass size out of this object at +0x50/+0x54 and both eyes render at 3072,
    // so requiring a full-size object rejects a recycled or half-built one.
    uint32_t wh[2] = {0, 0};
    if (!cloud_cb_raw_copy(wh, reinterpret_cast<const uint8_t*>(pair[0]) + 0x50, 8) ||
            wh[0] < 2048 || wh[1] < 2048)
        return false;
    out[0] = pair[0];
    out[1] = 0;                                             // no refcount, on purpose
    return true;
}

// Returns the viewData whose +0x168 was written, or 0. `saved` takes what was there.
uintptr_t comp_lend_install(void* work_context, uint64_t saved[2]) {
    if (!CyberpunkVR_CompLendScoped || !work_context) return 0;
    if (!g_viewdata_get && g_exe_base)
        g_viewdata_get = reinterpret_cast<ViewDataGetterFn>(g_exe_base + 0x1ED930);
    if (!g_viewdata_get) return 0;
    uintptr_t vd = 0;
    __try { vd = static_cast<uintptr_t>(g_viewdata_get(work_context)); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return 0; }
    if (!vd) return 0;
    uint64_t lent[2];
    if (!comp_lend_fetch(lent)) {
        InterlockedIncrement64(
            reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCompScopedSkips));
        return 0;
    }
    if (!cloud_cb_raw_copy(saved, reinterpret_cast<const uint8_t*>(vd) + 0x168, 16)) return 0;
    if (saved[0] || saved[1]) return 0;          // it has one of its own -- never displace that
    if (!cloud_cb_raw_copy(reinterpret_cast<uint8_t*>(vd) + 0x168, lent, 16)) return 0;
    InterlockedIncrement64(
        reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCompScopedLends));
    return vd;
}

// Puts back exactly what was found, which by the guard above is always a zeroed pair.
void comp_lend_restore(uintptr_t vd, const uint64_t saved[2]) {
    if (!vd) return;
    cloud_cb_raw_copy(reinterpret_cast<uint8_t*>(vd) + 0x168, saved, 16);
}

// THE DrawComposition DETOUR IS GONE, NOT MERELY GATED.
//
// It existed to serve CullReuseMode 5 -- lending MAIN's block list to the second view for one node
// call -- and that mode is off and unused; mode 5 was named nowhere else but here. So the whole body
// was dead, and its only remaining effect was AN EXTRA CALL TO THE ENGINE'S viewData ACCESSOR on every
// DrawComposition dispatch of BOTH views, for a result nobody read. That accessor is not a pure getter:
// the composition-lend note above records the provider doing InterlockedExchange(set+8, 0) with a
// ONE-SHOT per-frame flag, and the node dispatcher records that asking it again "once per composition
// dispatch, MAIN included -- is what produced the same crash three times running", an access violation
// reading 0x68 of a null object.
//
// Gating it on its own switch (which is what the parked render WIP 6f0af241 did) would have left a hook
// on the hottest composition path to answer a question already answered. It is deleted instead, so
// DRAWCOMP_RVA carries no detour at all -- which matters more now that xr_comp_lend_set makes the
// SECOND view run that node too.
//
// One consequence, stated rather than left to be discovered: this was the ONLY writer of
// g_main_block_v5, so CyberpunkVR_LightBlockLend can no longer lend anything even when armed. Its own
// note already records that lend crashing and calls viewData+0x168 unusable for it, so nothing of value
// is lost -- but if that mechanism is ever revived it needs a source for the pointer again.

// ---- registered where they are defined -----------------------------------------------------
// AN ELEMENT WITH NO OWNER IN THIS VIEW CONTRIBUTES NO DRAW KEYS.
//
// This is the entry to the whole subtree every fault under the composition lend has come from, and it
// is where the null enters. sub_1401ECBE0 walks MAIN's composition-element list and, for one element
// kind, does:
//
//     1401ECE2A  mov  r9, [rbx+28h]     ; the element's own owner field -- null for this view
//     1401ECE8E  call sub_1403C6970     ; r9 arrives as the 4th parameter
//     1401ECE93  jmp  loc_1401ECD03     ; ...and the caller NEVER READS THE RESULT
//
// From there it reaches the leaves untested, and both faults measured with the lend armed are that:
//
//     exe+0x3C6F5C   read at 0x70   `v3 = *(a1 + 0x70)`, a1 == 0
//     exe+0x2F098B   read at 0x68   a byte spin-lock acquired on *(null + 0x68)
//
// sub_1403C6EF8 makes the link explicit -- it calls sub_1403C6F5C(a3, ...) with a3 being this
// function's 4th parameter -- so guarding here covers the subtree instead of answering leaf by leaf,
// which is what the second fault showed to be a losing game.
//
// Returning early is legal, not merely convenient: the caller discards the return value on the very
// next instruction, and the effect is that this element emits no draw keys. That is the correct answer
// for an element the view has no owner for -- it could not have been drawn correctly anyway. It is the
// list-is-MAIN's problem solved as a FILTER at consumption rather than by rebuilding the list, which is
// the only other route this branch's own record allowed.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCompElemSkips = 0;

using CompElemKeysFn = __int64 (__fastcall*)(__int64, void*, unsigned int, __int64, __int64);
static CompElemKeysFn g_orig_comp_elem_keys = nullptr;

static __int64 __fastcall Detour_CompElemKeys(__int64 a1, void* a2, unsigned int a3,
                                             __int64 a4, __int64 a5) {
    if (!a4 && CyberpunkVR_CompLendSet) {
        InterlockedIncrement64(
            reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCompElemSkips));
        return 0;
    }
    return g_orig_comp_elem_keys(a1, a2, a3, a4, a5);
}
CVR_DETOUR("[complend] composition element draw keys sub_1403C6970", COMP_ELEM_KEYS_RVA,
           Detour_CompElemKeys, g_orig_comp_elem_keys)

// THE COMPOSITION LEND'S ONE NULL, ANSWERED THE WAY THE FUNCTION ITSELF ANSWERS IT.
//
// With CyberpunkVR_CompLendSet armed the second view runs CRenderNode_DrawComposition against an
// element list assembled for MAIN, and one element in it has no owner for this view. Measured twice,
// a year apart in this branch's own record and again today with the finished-frame copy working:
//
//     0xC0000005  read at 0x70,  FAULT exe+0x3C6F5C,  Rcx = 0
//     stack: exe+0x1ECE93, exe+0x1ED03C   (the element walk, sub_1401ECFDC -> sub_1401ECBE0)
//
// The null is loaded at exe+0x1ECE2A -- `mov r9, [rbx+28h]`, the element record's own field -- and
// handed down three frames untested, arriving as this function's first argument. Decompiled, the
// function is:
//
//     v3 = *(a1 + 0x70);                       <- the fault, on the FIRST instruction
//     *(a2) = 0; *(a2+8) = 0; *(a2+16) = 0;    <- the output is zeroed unconditionally
//     if (v3) { ...fill a2 from v3... }
//     return a2;
//
// So it ALREADY has a "nothing to compute" answer and takes it whenever *(a1+0x70) is null: zero the
// twenty output bytes and return a2. Nothing here is invented -- a1 == 0 is given the same answer the
// engine gives one dereference later, which is why this is a guard and not a behaviour change.
//
// Gated on our own switch, per this project's rule for hooks on a shared leaf: seven callers across the
// image reach this function, and none of them can be affected, because with the lend off the guard
// never runs at all. The counter increments only when it fires, so it costs nothing when it does not.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCompElemNulls = 0;

using CompElemRectFn = __int64 (__fastcall*)(__int64, __int64, unsigned short*);
static CompElemRectFn g_orig_comp_elem_rect = nullptr;

static __int64 __fastcall Detour_CompElemRect(__int64 a1, __int64 a2, unsigned short* a3) {
    if (!a1 && CyberpunkVR_CompLendSet) {
        if (a2) {
            *reinterpret_cast<uint64_t*>(a2)        = 0;
            *reinterpret_cast<uint64_t*>(a2 + 8)    = 0;
            *reinterpret_cast<uint32_t*>(a2 + 16)   = 0;
        }
        InterlockedIncrement64(
            reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCompElemNulls));
        return a2;
    }
    return g_orig_comp_elem_rect(a1, a2, a3);
}
CVR_DETOUR("[complend] composition element rect sub_1403C6F5C", COMP_ELEM_RECT_RVA,
           Detour_CompElemRect, g_orig_comp_elem_rect)

// One line every five seconds while the lend is armed, so the two guards can be told apart without a
// debugger: skips are elements filtered at the subtree entry, nulls are the leaf guard catching one
// that arrived by some other route. Off with the lend, and it is a throttled log rather than a probe on
// a hot path.
void complend_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (!CyberpunkVR_CompLendSet || (s_last && now - s_last < 5000)) return;
    s_last = now;
    Log("[complend] set=%u lends=%llu skips=%llu | elem: skipped=%llu leafNulls=%llu\n",
        CyberpunkVR_CompLendSet,
        (unsigned long long)CyberpunkVR_DebugCompLendWrites,
        (unsigned long long)CyberpunkVR_DebugCompLendSkips,
        (unsigned long long)CyberpunkVR_DebugCompElemSkips,
        (unsigned long long)CyberpunkVR_DebugCompElemNulls);
}


CVR_DETOUR("[light] RenderLightBuffers sub_14077D308 (block-list lend)", LIGHTBUFFERS_RVA, Detour_LightBuffers, g_orig_lightbuffers)

}  // namespace detail
}  // namespace cvr
