#include "Utils/DebugGate.hpp"
#include "Stereo/RenderParity.hpp"
#include "Render/OpticsProbe.hpp"
#include "Render/PhysicalOpticsShaders.hpp"
// NodeDispatch -- the one function every frame-graph node passes through, and the observers hung on it.
//
// THIS IS THE HUB. Detour_NodeDispatch is called once per node per view, and from it thirty-one other
// functions in this module get their chance to look at, count, permit, skip or redirect that node. It
// was extracted LAST on purpose: everything else had to have a home first, or this file would have
// pulled the whole module back into one place through its include list.
//
// WHAT IT DECIDES, in the order it decides it:
//
//   * which view this node belongs to -- the single comparison against the VRCAM identity hash that
//     the entire stereo path rests on
//   * whether the view's capability mask permits the node, and whether to grant a bit it lacks
//   * whether to skip the node because the other view already produced what it makes
//   * whether to record it: the per-node timing, the census, the sky and cloud snapshots
//
// THE SKIP AND THE GRANT ARE THE DANGEROUS PART, and the rule is written where they are: skipping a
// producer while leaving its feature bit SET is deliberate, because consumers below must still run and
// read what the first view produced. Clearing the bit instead makes the second eye lose the effect. That
// asymmetry is the single most expensive thing this module has learned.
//
// The offsets at the top (view-active flag, QPC timestamp, the CALLER1-only stage word) are read from
// the node's work context. They are constants because they were found by disassembly, and they are
// commented with what each holds because a bare offset is unverifiable a year later.
//
// Detour_ViewFeatureCheck sits here rather than with the frame graph: it is asked per NODE, not per
// build, and the answer it gives is what the dispatcher acts on.

#include "Stereo/SyncStereo.hpp"
#include "Framegen/Framegen.hpp"
#include "Framegen/Inputs.hpp"
#include "Framegen/NativeInputs.hpp"
#include "Camera/PoseIdentity.hpp"
#include "Render/ViewCameraLayout.hpp"
#include "Stereo/AutoGrassReadiness.hpp"
#include "Utils/MemorySafe.hpp"
#include "Utils/StereoLog.hpp"
#include "Stereo/VrcamConfig.hpp"   // vrcam.json access + CName hashing, shared with the launcher
#include "Render/ColorBlit.hpp"   // HUD debug overlay on the mirror image
#include <bit>
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
#include "Render/NativePreparation.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"
#include "Stereo/DetourRegistry.hpp"

namespace cvr {
namespace detail {

// 0 = MAIN, 1 = the second eye, -1 = a view that is neither (reflection-probe faces) or unknown.
thread_local int32_t t_view_side = -1;

constexpr uintptr_t OFF_B_1C0     = 0x1C0;  // byte (view-active flag)
constexpr uintptr_t OFF_QPC_2B8   = 0x2B8;  // qword (QPC timestamp)
constexpr uintptr_t OFF_S_348     = 0x348;  // dword (CALLER1-only scalar)

// Renderer global + view-state (render_camera_RE/STATE.md, verified addresses).
// renderer = *(qword_143BBCAE8); shared view-state = renderer + 0x4658.
// RVA_RENDERER_GLOBAL moved to Stereo/StereoInternal.hpp.
constexpr size_t VIEW_STATE_SNAPSHOT_OFFSET = 0x20;
constexpr size_t VIEW_STATE_SNAPSHOT_SIZE = 0x488;    // through last-frame token at +0x4A0
// Never wait for Present while holding the FG serialization lock. The 100 ms
// diagnostic barrier collapses stereo throughput to roughly 10 FPS whenever
// the offscreen/right graph intentionally has no desktop Present.
constexpr bool HAS_DXGI_PRESENT_OBSERVER = false;

uint8_t*               g_exe_base   = nullptr;

// Retained globals still referenced by the kept node dispatcher, record_node_dispatch and init
// (the rest of the old Type-A globals block was orphaned and removed).
using WaitOnAddressFn = BOOL (WINAPI *)(volatile VOID*, PVOID, SIZE_T, DWORD);
using WakeByAddressAllFn = VOID (WINAPI *)(PVOID);
WaitOnAddressFn         g_wait_on_address = nullptr;
WakeByAddressAllFn      g_wake_by_address_all = nullptr;
NodeDispatchFn          g_node_dispatch_orig = nullptr;
std::atomic<bool>       g_node_dispatch_hooked{false};

static bool is_vrcam_copy_to_texture(uintptr_t* node, uint8_t* work_context) {
    if (!node || !work_context) return false;
    __try {
        const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(work_context + 0x18);
        if (!ctx || *reinterpret_cast<uint64_t*>(ctx + 0x28) !=
                g_vrcam_ctx_key) {
            return false;
        }
        const uintptr_t vtable = *node;
        const uintptr_t work = *reinterpret_cast<uintptr_t*>(vtable + 8);
        const bool hit = work == reinterpret_cast<uintptr_t>(g_exe_base) +
            RENDER_FINAL2D_WORK_RVA;
        if (hit) CVR_DIAGNOSTIC(++CyberpunkVR_DebugMirrorCopyNodeHits);
        return hit;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

// ---- per-node CPU audit (phase 0) ------------------------------------------
// Successor of the removed record_node_dispatch audit: instead of exec ORDER it
// accumulates per-work-fn CPU TIME, split main vs vrcam. Open-addressing table
// keyed by work-fn RVA; dumped+reset on demand (overlay button) to cyberpunkvrport_stereo.log.
// Identifies exactly which node types cost CPU twice for simultaneous stereo.
// ProfNode moved to Stereo/StereoInternal.hpp.
ProfNode g_prof_nodes[512];      // ~few hundred distinct work fns in practice
// First-seen order counters. The graph is stable frame to frame, so first-seen order ==
// dispatch order, which is what makes the dump diffable against the older audits.
std::atomic<uint32_t> g_prof_ord_main{0};
std::atomic<uint32_t> g_prof_ord_vrcam{0};
// The per-node profiler accounting moved to src/Stereo/Profiler.cpp.
// The HUD capability grant moved to src/Stereo/Hud.cpp.
// The per-node profiler accounting moved to src/Stereo/Profiler.cpp.

// Answer the per-view capability question for the HUD node, and for nothing else.
//
// Ordered so the common case costs two loads: every other node on every other view goes straight
// through to the original. We only look further when the second eye is the one asking, and we
// never turn a YES into a NO -- only the specific NO that keeps the HUD out of the second eye.
// ---- the per-view RenderMask ---------------------------------------------------------------
//
// sub_14021BE28(wc, desc) is neither a private HUD gate nor an opaque "capability": every
// descriptor it is ever handed is a NAMED render-mask entry, registered at startup by a
// one-line function. The registrar for the one that matters here is
//     sub_1400F76B0:  "Rendering/RenderMask/DistantLights"  ->  word_143C1CDF0
// and the test passes when the view's 32-qword mask at view+0x18A0 is a superset of the
// descriptor's words. NOTE THE +8: it compares mask[i] against desc[i+1], so the required
// words begin at descriptor+8. Reading them from descriptor+0 -- which the first grant did --
// ORs the wrong words in and the test still fails. That, not the engine, is why "granting the
// capability changed nothing".
//
// Mapping the measured refusals onto the name table settles the unlit street lamps:
//     77CED4 ClusteredLightsCull + 77D308 RenderLightBuffers  ->  RenderMask/DistantLights
//     77D214 AutoSpawnOnTerrain  + 153844 RenderShadowCascade ->  RenderMask/AutoGrass
//     1EE760 DrawHUD                                          ->  RenderMask/HUD
// and the Nsight capture agrees end to end: with DistantLights present MAIN clears the 20-byte
// argument buffer Resource_1359, fills it with a 1964-group PipelineState_597 dispatch, binds a
// 36-index unit cube and issues CommandSignature_81 -> DrawIndexedInstanced(36, 1821), i.e.
// 1821 local-light proxy volumes into the lighting target. The RTT view is refused all of it,
// so its lamps light nothing while their emissive surface still renders -- exactly the report.
//
// Only real render-mask categories belong in this table. Half of the refusal list is
// `Rendering/Debug/...` -- distant-shadow previews, chrome balls, probe overlays -- and turning
// those on for the second eye is not a fix, it is a debug overlay. The earlier blanket grant
// (CapGrant 2) did turn them on.
// struct RenderMaskEntry now lives in Stereo/StereoInternal.hpp: FrameGraph.cpp reads this table.
const RenderMaskEntry kRenderMasks[] = {
    { "DistantLights",       0x3C1CDF0 },   // bit 0 -- the unlit lamps           [on by default]
    { "AutoGrass",           0x3C1D120 },   // bit 1 -- terrain scatter + its cascade
    { "Foliage",             0x3C1D010 },   // bit 2
    { "Decals",              0x3C1CBD0 },   // bit 3
    { "Terrain",             0x3C1CF00 },   // bit 4
    { "EnvProbes",           0x3C1D230 },   // bit 5
    { "Emissive",            0x3C1D340 },   // bit 6
    { "LightChannels",       0x3C1D450 },   // bit 7
    { "Fog",                 0x3C1D560 },   // bit 8
    { "Lights",              0x3C1CCE0 },   // bit 9
    { "ClearLighting",       0x3C1D780 },   // bit 10
    { "GameplayPostProcess", 0x3C1C9B0 },   // bit 11 -- scanner/focus tint etc  [on by default]
    { "HUD",                 0x3C1C8A0 },   // bit 12 -- we composite the HUD ourselves: leave off
    // THE OTHER FOURTEEN. The engine registers 27 of these and this table knew half, which is a
    // blind spot rather than a choice: a category nobody listed cannot be reported as missing, and
    // "granting the capability changed nothing" has been said in this project about a name that was
    // never in the list. They are appended (the grant is a bitmask over these indices, so appending
    // leaves every existing bit meaning what it did) and default to REPORT ONLY -- the report says
    // which view lacks what, and only then is granting an argument.
    //
    // Added 2026-08-17 while chasing cloth flags that do not wave in the second eye and do in MAIN.
    // A flag is skinned cloth, so GeometrySkinned is the first one to look at.
    { "GeometrySkinned",     0x3C1BBE0 },   // bit 13 -- skinned meshes: characters, and cloth
    { "GeometryStatic",      0x3C1BAD0 },   // bit 14
    { "GeometryProxies",     0x3C1BCF0 },   // bit 15
    { "DepthPrepass",        0x3C1BE00 },   // bit 16
    { "GBuffer",             0x3C1BF10 },   // bit 17
    { "GBufferLate",         0x3C1C020 },   // bit 18
    { "Forward",             0x3C1C240 },   // bit 19
    { "ForwardNoTXAA",       0x3C1C350 },   // bit 20
    { "Particles",           0x3C1CAC0 },   // bit 21
    { "Unlit",               0x3C1C680 },   // bit 22
    { "WeaponPlane",         0x3C1C130 },   // bit 23 -- the first-person weapon plane
    { "TopDownCarProxy",     0x3C1C790 },   // bit 24
    { "DebugDraw",           0x3C1C570 },   // bit 25
    { "Discarded",           0x3C1C460 },   // bit 26
};
const uint32_t kRenderMaskCount =
    static_cast<uint32_t>(sizeof(kRenderMasks) / sizeof(kRenderMasks[0]));

// THE VIEW BITSET IS NOT ONE CONTIGUOUS BITSET -- corrected 2026-07-31 by static reverse.
// The live diff named one clean MAIN-set/VRCAM-zero run in the graph context, 1870-1873{1},
// and the old note here read that as feature bit (0x1870-0x17D0)/8*64 = 1280. It is not.
// Every one of the 993 call sites of the feature test sub_14033E358 passes a bit in 0..91 --
// nothing reads 1280, and granting it did nothing on screen, as it could not. So 0x17D0 holds
// f0/f1 and the region up to the mask at 0x18A0 is other per-view state, not more bits.
// view+0x1870 remains an unexplained MAIN-only dword; identify its reader before writing it.


// One bit per row above.
//
// bit 0  DistantLights       -- local lamps cast no light in the second eye without it.
// bit 11 GameplayPostProcess -- the scanner's green screen tint, and Sandevistan / Kerenzikov /
//        focus mode / cyberspace with it. CRenderNode_GameplayPostFX (0x414DFC) is nothing but
//        this gate: `if (sub_14021BE28(wc, word_143C1C9B0)) do_the_work();`. The node audit had
//        already measured the shape of it -- MAIN 0.0134 ms vs VRCAM 0.0005 ms, i.e. dispatched
//        and instantly bailed, the same signature ScreenSpaceRain had before the wetness fix.
//
// AutoGrass (bit 1) is the remaining real difference and is left OFF: it makes the second view
// re-issue the terrain-scatter batches in the G-buffer and in both shadow cascades, which is a
// per-frame cost nobody has asked for yet.
// bit 1 AutoGrass added 2026-07-30: the second eye had bushes but no grass. It was left off
// deliberately -- it makes the view re-issue the terrain-scatter batches in the G-buffer AND
// in both shadow cascades -- on the grounds that nobody had reported it missing. Someone has.
//
// AND IT IS NOT THE SHADOW FLICKER, measured 2026-07-31. With the grant taken back to the
// point where the second view was refused the terrain scatter outright -- census read
// PrepareAutoSpawn V=0/5337, i.e. no grass in that eye at all -- the shadows still flickered.
// Do not re-suspect this bit. The symptom is two shadow SETS alternating, it is specific to
// where the head is standing and pointing, and grass is not in it.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_RenderMaskGrant =
    (1u << 0) | (1u << 1) | (1u << 11);
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRenderMaskGrants = 0;

// 1 = NEVER declare a StateBefore for a resource this port does not own unless that state was
// observed, for that exact resource, on this thread. 0 = the old behaviour, which falls back
// on a seed or a fixed guess. Kept switchable only so the two can be compared on the picture;
// the whole point of the change is that the guess is what the driver crashes on.
// DEFAULT BACK TO 0, ON THE PICTURE, MINUTES AFTER IT SHIPPED AT 1: it took stereo away
// entirely. The observation flag is only ever set inside hk_ResourceBarrier, so a resource the
// engine does not transition again after our seed never counts as observed -- and then the eye
// capture is refused on EVERY frame instead of the rare wrong one. Refusing to guess is right;
// refusing to copy is not the way to do it, and the fix belongs in observing more, not copying
// less. Kept as a live switch so the two can still be compared, and so the seed-versus-observed
// distinction that the flags now carry is not thrown away.
// Defined in ViewReuse.cpp: the unscoped composition lend. Named here because the viewData fill
// further down is what installs it, so that fill has to run whenever the lend is armed.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_CompLendSet;

extern "C" __declspec(dllexport) int32_t  CyberpunkVR_NoStateLies = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugForeignStateRefusals = 0;
std::atomic<uintptr_t> g_vrcam_ctx_seen{0};
// render_mask_report is declared in Stereo/StereoInternal.hpp; its definition moved to another file.

// The mask lives on the view object, which outlives the frame, so in practice this is one-shot
// per view: after the first pass nothing is missing and the loop finds no work. Re-checking on
// every node dispatch is what keeps it correct across a view being recreated.
static std::array<uint64_t,32> g_world_render_mask{};
static std::array<uint64_t,32> g_world_render_source{};
static std::mutex g_world_render_mask_mutex;
// Bumped under the mutex whenever g_world_render_mask is rebuilt, so readers can keep a
// per-thread copy and take the lock only when it has changed.
static std::atomic<uint64_t> g_world_render_mask_gen{0};

// Share the named world-render categories, not the eye's HUD/debug ownership.
// Build this snapshot only when VRCAM's mask changes, then apply its 32 words.
static void share_world_render_mask_locked(uintptr_t ctx,bool eye) {
    std::array<uint64_t,32> have{};
    if(!cloud_cb_raw_copy(have.data(),reinterpret_cast<const void*>(ctx+0x2688),sizeof(have)))return;
    std::lock_guard lock(g_world_render_mask_mutex);
    if(eye) {
        if(have==g_world_render_source)return;
        g_world_render_source=have;g_world_render_mask.fill(0);
        for(uint32_t k=0;k<kRenderMaskCount;++k) {
            if(k==12 || k==25 || k==26)continue;
            std::array<uint64_t,32> need{};
            if(!cloud_cb_raw_copy(need.data(),g_exe_base+kRenderMasks[k].desc_rva+8,sizeof(need)))continue;
            bool enabled=true;
            for(size_t i=0;i<need.size();++i)enabled&=(have[i]&need[i])==need[i];
            if(enabled)for(size_t i=0;i<need.size();++i)g_world_render_mask[i]|=need[i];
        }
        g_world_render_mask_gen.fetch_add(1,std::memory_order_release);
    } else {
        for(size_t i=0;i<have.size();++i)have[i]|=g_world_render_mask[i];
        cloud_cb_raw_copy(reinterpret_cast<void*>(ctx+0x2688),have.data(),sizeof(have));
    }
}

// THE SAME RESULT WITHOUT THE LOCK. This runs on every VRCAM node and every scope node, so the
// mutex above made the render jobs of both views queue on one lock, and the scope path rewrote
// all 256 bytes of the engine's view mask on every node even when nothing was missing -- racing
// the engine's own writes to those words. The eye path now compares against the last snapshot
// this thread saw and locks only when the eye's mask changed; the scope path keeps a per-thread
// copy of the shared mask (refreshed by generation) and ORs in only the words that are missing.
// 0 = the original locked path, kept for A/B.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_MaskShareFast = 1;
// Cost instrument: 1 = accumulate QPC ticks and calls of every share below.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_MaskShareTiming = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_MaskShareTicks = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_MaskShareCalls = 0;
// The whole per-node pre-work of a scope node (grants, weapon-plane clear, viewData fill).
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_OpticsPreworkTicks = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_OpticsPreworkCalls = 0;

static void or_missing_mask_words(uintptr_t ctx,const uint64_t* mask,const uint64_t* have) {
    __try {
        uint64_t* dst = reinterpret_cast<uint64_t*>(ctx + 0x2688);
        for (size_t i = 0; i < 32; ++i) {
            const uint64_t missing = mask[i] & ~have[i];
            if (missing) dst[i] |= missing;
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}

static void share_world_render_mask_fast(uintptr_t ctx,bool eye) {
    thread_local std::array<uint64_t,32> t_source{};
    thread_local std::array<uint64_t,32> t_mask{};
    thread_local uint64_t t_gen = ~0ull;
    std::array<uint64_t,32> have{};
    if(!cloud_cb_raw_copy(have.data(),reinterpret_cast<const void*>(ctx+0x2688),sizeof(have)))return;
    if(eye) {
        if(have==t_source)return;
        share_world_render_mask_locked(ctx,true);
        t_source=have;
        return;
    }
    const uint64_t gen=g_world_render_mask_gen.load(std::memory_order_acquire);
    if(gen!=t_gen) {
        std::lock_guard lock(g_world_render_mask_mutex);
        t_mask=g_world_render_mask;
        t_gen=g_world_render_mask_gen.load(std::memory_order_relaxed);
    }
    or_missing_mask_words(ctx,t_mask.data(),have.data());
}

static void share_world_render_mask(uintptr_t ctx,bool eye) {
    if(!CyberpunkVR_MaskShareTiming) {
        if(CyberpunkVR_MaskShareFast)share_world_render_mask_fast(ctx,eye);
        else share_world_render_mask_locked(ctx,eye);
        return;
    }
    LARGE_INTEGER t0{},t1{};
    QueryPerformanceCounter(&t0);
    if(CyberpunkVR_MaskShareFast)share_world_render_mask_fast(ctx,eye);
    else share_world_render_mask_locked(ctx,eye);
    QueryPerformanceCounter(&t1);
    InterlockedAdd64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_MaskShareTicks),t1.QuadPart-t0.QuadPart);
    InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_MaskShareCalls));
}

static void render_mask_grant(uintptr_t ctx,bool eye=true) {
    const uint32_t want = CyberpunkVR_RenderMaskGrant;
    if (!ctx || !want || !g_exe_base) return;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    // The OR of the wanted categories' descriptors, built once per value of `want` on each thread:
    // ORing the union into the mask is the same as ORing every category in turn, and it is one
    // 32-word pass per node instead of one per wanted category, read again from the descriptors.
    thread_local uint32_t t_want = 0;
    thread_local uint64_t t_union[32] = {};
    __try {
        if (t_want != want) {
            for (int i = 0; i < 32; ++i) t_union[i] = 0;
            for (uint32_t k = 0; k < kRenderMaskCount; ++k) {
                if (!(want & (1u << k))) continue;
                const uint64_t* need =
                    reinterpret_cast<const uint64_t*>(base + kRenderMasks[k].desc_rva) + 1;
                for (int i = 0; i < 32; ++i) t_union[i] |= need[i];
            }
            t_want = want;
        }
        uint64_t* have = reinterpret_cast<uint64_t*>(ctx + 0x2688);
        bool changed = false;
        for (int i = 0; i < 32; ++i) {
            const uint64_t missing = t_union[i] & ~have[i];
            if (missing) { have[i] |= missing; changed = true; }
        }
        if (changed)
            CVR_DIAGNOSTIC(InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugRenderMaskGrants)));
    } __except (EXCEPTION_EXECUTE_HANDLER) { t_want = 0; return; }
    share_world_render_mask(ctx,eye);
    if(eye)g_vrcam_ctx_seen.store(ctx, std::memory_order_release);
}

// THE SCOPE CAMERA MUST NOT SEE THE PLAYER'S OWN FIRST-PERSON PLANE. Its RTT view is a camera on
// the player, and the engine grants it RenderMask/WeaponPlane every frame like any view (native
// word 11 = 0xcf6e3b, bit 0 set): the gun and arms were drawn into the magnified image with the
// weapon plane's own projection -- the barrel rising to the image centre at 4.4x, far above where
// its real geometry would sit. Measured live 2026-10-13: holding the WeaponPlane-only bits clear
// in the view mask removes the gun from the raw camera image and nothing else. Cleared at every
// scope node, after the grants above, since the engine rewrites the mask once per frame.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_OpticsHideWeaponPlane = 1;
// Bits WeaponPlane requires that no other category does (the categories share their view-type
// bits; clearing those would refuse every pass).
static const std::array<uint64_t,32>& weapon_plane_only_bits() {
    static const std::array<uint64_t,32> only = [] {
        std::array<uint64_t,32> weapon{},others{},result{};
        constexpr uint32_t weaponIndex = 23;
        if (!cloud_cb_raw_copy(weapon.data(), g_exe_base + kRenderMasks[weaponIndex].desc_rva + 8, sizeof(weapon)))
            return result;
        for (uint32_t k = 0; k < kRenderMaskCount; ++k) {
            std::array<uint64_t,32> need{};
            if (k == weaponIndex ||
                !cloud_cb_raw_copy(need.data(), g_exe_base + kRenderMasks[k].desc_rva + 8, sizeof(need))) continue;
            for (size_t i = 0; i < need.size(); ++i) others[i] |= need[i];
        }
        int bits = 0;
        for (size_t i = 0; i < result.size(); ++i) {
            result[i] = weapon[i] & ~others[i];
            bits += std::popcount(result[i]);
        }
        log("[optics] scope view hides the weapon plane: %d mask bit(s)", bits);
        return result;
    }();
    return only;
}
static void optics_hide_weapon_plane(uintptr_t ctx) {
    if (!ctx || !CyberpunkVR_OpticsHideWeaponPlane || !g_exe_base) return;
    const auto& only = weapon_plane_only_bits();
    __try {
        uint64_t* have = reinterpret_cast<uint64_t*>(ctx + 0x2688);
        for (size_t i = 0; i < only.size(); ++i)
            if (have[i] & only[i]) have[i] &= ~only[i];
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}

// Capability-refusal census. sub_14021BE28 is not the HUD's private gate: ClusteredLightsCull
// runs its whole body only `if (... || sub_14021BE28(a2, &word_143C1CDF0) != 0)`, and other
// nodes use it with their own descriptors. DrawHUD's refusal was found by hand and cost a
// session; enumerating every (node, descriptor) the engine denies the second view is the same
// work done once, for all of them.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CapCensus = 1;   // OFF: [cap] per-view gate refusals -- the first thing to switch on for a new "the second eye is missing X"
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCapDenies = 0;
struct CapDeny { uint32_t node_rva, desc_rva; uint64_t hits; };
static std::array<CapDeny, 32> g_cap_deny{};
static uint32_t g_cap_deny_n = 0;
static std::mutex g_cap_deny_mtx;

static void cap_census_note(uintptr_t work, uintptr_t required) {
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    if (!base) return;
    const uint32_t nrva = (work > base) ? static_cast<uint32_t>(work - base) : 0;
    const uint32_t drva = (required > base) ? static_cast<uint32_t>(required - base) : 0;
    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCapDenies)));
    bool dump = false;
    {
        std::lock_guard<std::mutex> lk(g_cap_deny_mtx);
        uint32_t i = 0;
        for (; i < g_cap_deny_n; ++i)
            if (g_cap_deny[i].node_rva == nrva && g_cap_deny[i].desc_rva == drva) break;
        if (i == g_cap_deny_n) {
            if (g_cap_deny_n >= g_cap_deny.size()) return;
            g_cap_deny[g_cap_deny_n++] = { nrva, drva, 0 };
            dump = true;                       // a pair we have not seen -> report the table
        }
        ++g_cap_deny[i].hits;
        if (dump) {
            char line[900];
            int used = 0;
            line[0] = '\0';
            for (uint32_t k = 0; k < g_cap_deny_n; ++k)
                if (used < static_cast<int>(sizeof(line)) - 32)
                    used += snprintf(line + used, sizeof(line) - used, "node %X/desc %X ",
                                     g_cap_deny[k].node_rva, g_cap_deny[k].desc_rva);
            log("[cap] VRCAM capability refusals (%u distinct): %s", g_cap_deny_n, line);
        }
    }
}

// Granting = making the test pass HONESTLY, by OR-ing the bits the descriptor asks for into the
// view's own 32-qword mask at ctx+6304 -- exactly what fixed the HUD. Not the same thing as
// returning 1 from the gate: that leaves the mask short, so the next node to ask gets refused
// again and anything downstream that reads the mask still sees a crippled view.
//
// The measured refusals, by node:
//   77CED4 ClusteredLightsCull, 77D308 RenderLightBuffers   <- the reported unlit lights
//   77E610 ReflectionProbes, 786BCC RenderShadowmask, 153844 RenderShadowCascade,
//   6212EC DecoupledParticleLighting, 775ACC SetRenderTargetsMain, 774CF8 HistogramUpdate,
//   77120C GameplayPostFX, 77B638/77D214 AutoSpawnOnTerrain
// 0 = census only, 1 = the two light nodes (default: the smallest change that addresses the
// symptom), 2 = every refusal.
// SUPERSEDED by CyberpunkVR_RenderMaskGrant, and left at 0.
// Two things were wrong with granting here. It read the required words from descriptor+0 when
// the test compares against descriptor+8, so it never actually satisfied anything -- and it
// fired on EVERY refusal, which includes a dozen `Rendering/Debug/...` overlays the engine is
// right to withhold. The named table above grants one specific category from the engine's own
// descriptor. Kept only so the census can still be run with the grant off.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CapGrant = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCapGrants = 0;

static bool cap_grant_required(uintptr_t work_context, uintptr_t required) {
    if (!work_context || !required) return false;
    __try {
        const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(work_context + 0x18);
        if (!ctx) return false;
        uint64_t* have = reinterpret_cast<uint64_t*>(ctx + 0x2688);
        // +1 qword: sub_14021BE28 compares mask[i] against descriptor[i+1].
        const uint64_t* need = reinterpret_cast<const uint64_t*>(required) + 1;
        bool changed = false;
        for (int i = 0; i < 32; ++i) {
            const uint64_t missing = need[i] & ~have[i];
            if (missing) { have[i] |= missing; changed = true; }
        }
        return changed;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

// ---- WHICH FEATURE BITS THE SHADOW-MASK PASS ASKS ABOUT, and whether the two views answer alike -----
//
// The sun-shadow mask is the last part of the chain still unexplained: everything upstream is measured
// identical between the eyes (same casters, same cascade records, both cascades rendered by both views,
// same wind, same frame constants), and every remaining asymmetry in the small-block census is one of this
// port's own reuse knobs. Its pass, CRenderNode_RenderShadowmask, branches on feature bits --
//
//     if (feature(0x2C)) { if (feature(0x2D)) return; }      // 44 then 45: an early-out
//     if (feature(0x2C)) { ... feature(0x0E) ... }           // 44 then 14
//     ... feature(0x14), feature(0x0D) ...
//
// -- so if a bit answers differently for the two views, that eye takes a different path through the mask
// and the difference needs no exotic explanation. This measures the answers before anything is forced;
// forcing a bit whose meaning is unmeasured is what the cascade attempts cost.
//
// ANSWERED, IN GAMEPLAY, AND THE ANSWER IS NO. Every bit the pass asks about answers the same for both
// views, and the per-interval deltas are identical (408 each), so the two eyes run the pass at the same
// rate down the same branch -- the standing gap in the totals is a constant accumulated before gameplay:
//
//     bit9  M=10966/0  V=15809/0        both always YES (asked twice per pass)
//     bit44 M=0/16449  V=0/23712        both always NO  -- the early-out is never taken by either view
//     bit20 M=5483/0   V=7904/0         both always YES
//     bit13 M=5483/0   V=7904/0         both always YES
//     bit14 M=5483/0   V=7904/0         both always YES
//
// So the shadow-mask pass is not where the eyes diverge either. What is left is structural and is written
// up at CyberpunkVR_CheckerProbe in Grading.cpp: the mask is evaluated at HALF WIDTH on an interleaved
// pattern addressed in each eye's own screen space.
//
// THIS PROBE ALSO COST A RUN BY BEING ATTACHED TO THE WRONG FUNCTION, which is why the two are now named
// apart in EngineRvas.hpp: sub_14021BE28 answers about a render-mask DESCRIPTOR (that is the HUD/AutoGrass
// gate), sub_14033E358 answers about a BIT INDEX (that is what the nodes ask). A silent probe is not a
// negative result, and the yes/no counters are printed so an empty answer can be told from an unreached one.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_FeatBitProbe = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_FeatBitNode = RENDER_SHADOWMASK_RVA;

namespace {
struct FeatBit { uint32_t bit; uint64_t yes[2]; uint64_t no[2]; };
FeatBit g_featbits[32];
uint32_t g_featbits_n = 0;
std::mutex g_featbits_mtx;

void featbit_note(uint32_t bit, bool answer, bool vrcam) {
    const int v = vrcam ? 1 : 0;
    std::lock_guard<std::mutex> lk(g_featbits_mtx);
    uint32_t i = 0;
    for (; i < g_featbits_n; ++i) if (g_featbits[i].bit == bit) break;
    if (i == g_featbits_n) {
        if (g_featbits_n >= 32) return;
        g_featbits[g_featbits_n++] = FeatBit{bit, {0, 0}, {0, 0}};
    }
    if (answer) ++g_featbits[i].yes[v];
    else        ++g_featbits[i].no[v];
}

void featbit_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    char line[1000];
    int used = 0;
    line[0] = 0;
    std::lock_guard<std::mutex> lk(g_featbits_mtx);
    for (uint32_t i = 0; i < g_featbits_n; ++i) {
        const FeatBit& f = g_featbits[i];
        // A bit is interesting when the two views do not agree: one answers yes where the other says no.
        const bool m_yes = f.yes[0] > f.no[0];
        const bool v_yes = f.yes[1] > f.no[1];
        const char* mark = (m_yes != v_yes) ? " <-- DIFFERS" : "";
        if (used < static_cast<int>(sizeof(line)) - 80)
            used += snprintf(line + used, sizeof(line) - used,
                             "bit%u M=%llu/%llu V=%llu/%llu%s  ", f.bit,
                             (unsigned long long)f.yes[0], (unsigned long long)f.no[0],
                             (unsigned long long)f.yes[1], (unsigned long long)f.no[1], mark);
    }
    log("[featbit] node %X feature answers (yes/no per view): %s",
        CyberpunkVR_FeatBitNode, used ? line : "(none seen)");
}
}  // namespace

// The per-bit test itself. Hooked separately from Detour_ViewFeatureCheck because they are different
// engine functions: that one is sub_14021BE28 and answers about a render-mask DESCRIPTOR, this one is
// sub_14033E358 and answers about a BIT INDEX. The shadow-mask pass asks this one, which is why the first
// version of this probe -- attached to the other -- printed nothing at all.
using FeatureBitFn = uint8_t (__fastcall*)(uintptr_t, uintptr_t);
static FeatureBitFn g_orig_feature_bit = nullptr;
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_AutoGrassReadiness[4]{};

static uint8_t __fastcall Detour_FeatureBit(uintptr_t work_context, uintptr_t bit) {
    const uint8_t r = g_orig_feature_bit(work_context, bit);
    if (CyberpunkVR_FeatBitProbe && g_exe_base && bit < 64) {
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        if (t_current_node_work > base &&
                static_cast<uint32_t>(t_current_node_work - base) == CyberpunkVR_FeatBitNode) {
            featbit_note(static_cast<uint32_t>(bit), r != 0, t_vrcam_node_active);
            featbit_report();
        }
    }
    return r;
}
CVR_DETOUR("[featbit] per-bit feature test sub_14033E358", FEATURE_BIT_TEST_RVA, Detour_FeatureBit, g_orig_feature_bit)

// The scope view's first-person pass. The render-element pass driver (sub_14033DDA4) draws a pass
// flagged 0x20 -- the weapon plane -- only if THIS test passes for RenderMask/WeaponPlane.
// Clearing the bit in the view mask at each node was not enough: the view context outlives the
// frame and the engine rewrites its mask from another thread, so the bit was back by the time
// the node asked in some frames and the barrel blinked in the lens. The answer is given here,
// from the view the node renders.
static uint64_t optics_view_key() {
    static const uint64_t key = cvr::cname_hash("vr_optics_feed");
    return key;
}
static bool optics_refuses(uintptr_t work_context, uintptr_t required) {
    if (!g_exe_base || !CyberpunkVR_OpticsHideWeaponPlane ||
        required != reinterpret_cast<uintptr_t>(g_exe_base) + kRenderMasks[23].desc_rva) return false;
    const uint64_t optics = optics_view_key();
    __try {
        const uintptr_t ctx = *reinterpret_cast<const uintptr_t*>(work_context + 0x18);
        return ctx && *reinterpret_cast<const uint64_t*>(ctx + 0x28) == optics;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
uint8_t __fastcall Detour_ViewFeatureCheck(uintptr_t work_context, uintptr_t required) {
    if (optics_refuses(work_context, required)) return 0;
    const uint8_t r0 = g_view_feature_check_orig(work_context, required);
    if(t_vrcam_node_active && g_exe_base &&
       required==reinterpret_cast<uintptr_t>(g_exe_base)+kRenderMasks[1].desc_rva) {
        // Our AutoGrass grant must not turn a zero native buffer into a valid
        // resource. Two save-load dumps reached1F51F5 with handle0: decrementing
        // it produced indexFFFFFFFF and an out-of-bounds resource-table read.
        // Keep the persistent mask intact and recheck the current world's state
        // at each use, so grass resumes as soon as its buffer exists again.
        const auto ready=InspectAutoGrassInputs(work_context,ReadPtrSafe,ReadU32Safe);
        CVR_DIAGNOSTIC(++CyberpunkVR_AutoGrassReadiness[static_cast<unsigned>(ready.status)]);
        if(!ready)return 0;
    }
    if (!r0 && t_vrcam_node_active && CyberpunkVR_CapCensus)
        cap_census_note(t_current_node_work, required);
    if (!r0 && t_vrcam_node_active && CyberpunkVR_CapGrant && g_exe_base) {
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        const uint32_t nrva = (t_current_node_work > base)
                                  ? static_cast<uint32_t>(t_current_node_work - base) : 0;
        const bool wanted = (CyberpunkVR_CapGrant >= 2) ||
                            nrva == CLUSTERED_LIGHTS_CULL_RVA ||
                            nrva == RENDER_LIGHT_BUFFERS_RVA;
        if (wanted && cap_grant_required(work_context, required)) {
            CVR_DIAGNOSTIC(InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCapGrants)));
            return g_view_feature_check_orig(work_context, required);   // now it passes on merit
        }
    }
    if (!CyberpunkVR_HudInVrcam || !t_vrcam_node_active) return r0;
    const uint8_t r = r0;
    if (r) return r;
    const uintptr_t work = t_current_node_work;
    if (!work || !g_exe_base || work <= reinterpret_cast<uintptr_t>(g_exe_base)) return r;
    if (static_cast<uint32_t>(work - reinterpret_cast<uintptr_t>(g_exe_base)) != DRAWHUD_WORK_RVA) {
        return r;
    }
    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugHudGateDenied)));
    // DrawHUD asks twice (word_143C1C8A0 in the prologue, word_143C1C9B0 further in), so this
    // deliberately does not discriminate by descriptor -- inside the HUD node, on the second
    // eye, every capability refusal is the same refusal.
    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugHudGateForced)));
    static bool s_said = false;
    if (!s_said) {
        s_said = true;
        log("[hud] second-eye capability refusal overridden inside DrawHUD -- HUD should now "
            "render for VRCAM (set CyberpunkVR_HudInVrcam=0 to revert)");
    }
    return 1;
}

// A REFUSED WEAPON PLANE IS TAKEN OUT OF THE PASS LIST, NOT SKIPPED. The render-element pass driver
// sub_14033DDA4 (2.31: sub_14023A938; the node's work function, its return ignored) remembers each
// pass's flags BEFORE it asks whether a weapon-plane pass (flag 0x20) may draw. A refused one is
// skipped, and the next pass with the same flags takes the "same as the last pass" shortcut straight
// to the draw -- through the draw functor the skipped pass never built. With no pass before them that
// built one, the functor is empty: the call through null at +0x2093FC2 that took the game down when a
// camera scope came up (2026-10-26). The engine never refuses a view the weapon plane it has passes
// for; the port does (the scope view must not hold the player's own gun). So a node whose view is
// refused the weapon plane runs on a copy of itself whose list leaves those passes out. The driver
// reads the node at +24/+32/+40 (callbacks given the work context), +48 (passes, 48 bytes each,
// flags at +8) and +60 (count) only.
using PassDriverFn = void(__fastcall*)(uintptr_t node, uintptr_t work, uintptr_t args);
static PassDriverFn g_orig_pass_driver = nullptr;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_WeaponPassListsFiltered = 0;
constexpr uint32_t kPassEntry = 48, kPassFlags = 8, kWeaponPass = 0x20;
// The node's pass count when its list holds a weapon-plane pass, else 0.
static uint32_t weapon_pass_count(uintptr_t node, uintptr_t* list) {
    __try {
        *list = *reinterpret_cast<const uintptr_t*>(node + 48);
        const uint32_t count = *reinterpret_cast<const uint32_t*>(node + 60);
        if (!*list || count > 4096) return 0;
        for (uint32_t i = 0; i < count; ++i)
            if (*reinterpret_cast<const uint32_t*>(*list + i * kPassEntry + kPassFlags) & kWeaponPass) return count;
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return 0;
}
// The node's first 64 bytes into `copy` and its passes without the weapon-plane ones into `kept`
// (room for `count`); the number kept, or -1 if the node could not be read.
static int32_t copy_without_weapon_passes(uintptr_t node, uintptr_t list, uint32_t count, uint8_t* copy, uint8_t* kept) {
    __try {
        uint32_t n = 0;
        for (uint32_t i = 0; i < count; ++i) {
            const auto* entry = reinterpret_cast<const uint8_t*>(list + i * kPassEntry);
            if (!(*reinterpret_cast<const uint32_t*>(entry + kPassFlags) & kWeaponPass))
                std::memcpy(kept + size_t(n++) * kPassEntry, entry, kPassEntry);
        }
        std::memcpy(copy, reinterpret_cast<const void*>(node), 64);
        return static_cast<int32_t>(n);
    } __except (EXCEPTION_EXECUTE_HANDLER) { return -1; }
}
// The view key of the work context (ctx+0x28: 0 MAIN, else the RTT camera's), ~0 if unreadable.
static uint64_t pass_view_key(uintptr_t work) {
    __try {
        const uintptr_t ctx = *reinterpret_cast<const uintptr_t*>(work + 0x18);
        return ctx ? *reinterpret_cast<const uint64_t*>(ctx + 0x28) : ~0ull;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return ~0ull; }
}
static void __fastcall Detour_PassDriver(uintptr_t node, uintptr_t work, uintptr_t args) {
    uintptr_t list = 0;
    const uint32_t count = node ? weapon_pass_count(node, &list) : 0;
    if (!count || !g_exe_base || !work ||
        Detour_ViewFeatureCheck(work, reinterpret_cast<uintptr_t>(g_exe_base) + kRenderMasks[23].desc_rva)) {
        g_orig_pass_driver(node, work, args);
        return;
    }
    std::vector<uint8_t> kept(size_t(count) * kPassEntry);
    alignas(16) uint8_t copy[64];
    const int32_t n = copy_without_weapon_passes(node, list, count, copy, kept.data());
    if (n < 0) {
        g_orig_pass_driver(node, work, args);
        return;
    }
    *reinterpret_cast<uintptr_t*>(copy + 48) = reinterpret_cast<uintptr_t>(kept.data());
    *reinterpret_cast<uint32_t*>(copy + 60) = static_cast<uint32_t>(n);
    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_WeaponPassListsFiltered)));
    // Which views this happens to, once per view key: the scope view is the one the port refuses.
    static std::atomic<uint64_t> s_keys[8]{};
    const uint64_t key = pass_view_key(work);
    for (auto& slot : s_keys) {
        uint64_t seen = slot.load(std::memory_order_relaxed);
        if (seen == key + 1) break;
        if (!seen && slot.compare_exchange_strong(seen, key + 1)) {
            log("[optics] weapon-plane passes left out of a node's list: view key %016llX%s, %u of %u kept",
                static_cast<unsigned long long>(key), key == optics_view_key() ? " (scope view)" : "",
                static_cast<unsigned>(n), count);
            break;
        }
    }
    g_orig_pass_driver(reinterpret_cast<uintptr_t>(copy), work, args);
}
CVR_DETOUR("[optics] render-element pass driver sub_14033DDA4", 0x33DDA4, Detour_PassDriver, g_orig_pass_driver)

// Defined with the cloud block further down; called from here because the node dispatch is the
// EARLIEST point in a view's frame, and a hole has to be filled before its consumer runs. The
// cloud hook was too late: the wetness bytes landed (the diff stopped reporting the hole) but
// ScreenSpaceRain had already read zero and bailed.
// viewdata_fill_from_wc moved with the view-reuse family; declared in Stereo/StereoInternal.hpp.
// Worth knowing about this one: it appeared in a crash stack as a STALE frame once, and the
// investigation that cleared it turned on its call site being here rather than in the detour.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_ViewDataFixMask;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_FogMirrorMask;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_EnvMirrorMask;

// ---- per-view node census ---------------------------------------------------------------
// Which passes does the engine actually dispatch for each view? "VRCAM is missing effect X"
// is otherwise answered by breakpointing candidate nodes one at a time, which is slow and only
// ever tests the guesses one happened to make. This records the SET of work-fn RVAs each view
// runs and logs the difference, so the answer arrives in one pass over a live frame.
// (The two arrays have existed as exports for a long time but nothing ever filled them, which
// is why they always read back zero.)
extern "C" __declspec(dllexport) int32_t CyberpunkVR_NodeCensus = 1;
static std::mutex g_census_mtx;

static void node_census_add(uint32_t rva, bool vrcam) {
    uintptr_t* list = vrcam ? CyberpunkVR_DebugSecondaryNodeWorks
                            : CyberpunkVR_DebugMainNodeWorks;
    uint32_t& n     = vrcam ? CyberpunkVR_DebugSecondaryNodeUnique
                            : CyberpunkVR_DebugMainNodeUnique;
    std::lock_guard<std::mutex> lk(g_census_mtx);
    for (uint32_t i = 0; i < n; ++i)
        if (static_cast<uint32_t>(list[i]) == rva) return;
    if (n >= 256) return;
    list[n] = rva;
    ++n;
}

// The interesting output is the asymmetry, so log exactly that: RVAs one view dispatches and
// the other does not. Time-gated and one-shot-ish -- the sets converge within a second or two,
// after which the same two lines just repeat.
static void node_census_dump() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 20000) return;
    uint32_t mainRva[256], vrRva[256], mainN, vrN;
    {
        std::lock_guard<std::mutex> lk(g_census_mtx);
        mainN = CyberpunkVR_DebugMainNodeUnique;
        vrN   = CyberpunkVR_DebugSecondaryNodeUnique;
        for (uint32_t i = 0; i < mainN; ++i)
            mainRva[i] = static_cast<uint32_t>(CyberpunkVR_DebugMainNodeWorks[i]);
        for (uint32_t i = 0; i < vrN; ++i)
            vrRva[i] = static_cast<uint32_t>(CyberpunkVR_DebugSecondaryNodeWorks[i]);
    }
    if (!mainN || !vrN) return;      // wait until both views have been seen at all
    s_last = now;
    for (int pass = 0; pass < 2; ++pass) {
        const uint32_t* a = pass ? vrRva : mainRva;
        const uint32_t* b = pass ? mainRva : vrRva;
        const uint32_t  na = pass ? vrN : mainN, nb = pass ? mainN : vrN;
        char line[1024];
        int  used = 0, count = 0;
        line[0] = '\0';
        for (uint32_t i = 0; i < na; ++i) {
            bool shared = false;
            for (uint32_t j = 0; j < nb && !shared; ++j) shared = (b[j] == a[i]);
            if (shared) continue;
            ++count;
            if (used < (int)sizeof(line) - 16)
                used += snprintf(line + used, sizeof(line) - used, "%X ", a[i]);
        }
        log("[census] %s-only nodes: %d of %u | %s", pass ? "VRCAM" : "MAIN",
            count, na, count ? line : "(none)");
    }
}

// ---- the sky pass, as reversed -----------------------------------------------------------
//
// CRenderNode_RenderSkyScattering (sub_1407F86CC) only checks feature 35 and forwards to
// sub_1407F8714, and THAT is where the decision is:
//
//     v3 = *(QWORD*)(*(QWORD*)(a2+32) + 96);          // the sky manager
//     v4 = *(DWORD*)(sub_14033C248(a2) + 3988);       // viewData+0xF94, the AA/upscaler mode
//     v8 = 32 * *(BYTE*)(view + 5856);                // 32-byte record, INDEXED BY THE VIEW
//     if ( *(DWORD*)(v8 + v3 + 72) && !v4 || v7 ) { ...build the sky... }
//       ...
//     *(BYTE*)(v8+v3+80) = slot+1;      // one of six slots per frame
//     if (v7 || *(BYTE*)(v8+v3+80) >= 6) { ...publish, timestamp, reset the counter... }
//
// Two things follow, and both are measurable rather than arguable.
//
// The sky LUT is built AMORTISED -- six slots, one per frame, then published and the counter
// reset with an InterlockedExchange. That is shared mutable state with a work counter, the same
// shape as the GI clipmaps and the shadow cascades, and the same shape that has produced every
// "two versions alternating" symptom in this project. Whether the two views share it depends
// entirely on the byte at view+5856 (0x16E0): different index, private record; same index, they
// take turns filling one sky.
//
// And the gate reads viewData+0xF94 -- the very field StreamlineHistoryFix WRITES, mirroring
// MAIN's AA mode onto VRCAM. So our own fix is an input to the sky decision, which nobody knew.
//
// Report both, for both views. Reading only; nothing here changes engine state.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_SkyProbe = 1;   // answered: both views index sky record 0, AA mode 0
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugSkyIdxMain  = 0xFFFF;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugSkyIdxVrcam = 0xFFFF;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMainAaMode;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugVrcamAaMode;

// ---- the cloud lighting shader selector -----------------------------------------------------
//
// sub_1407F36D8 RenderVolumetricCloudsLighting, decompiled:
//     v5  = *(int**)(ctx + 7592);        // ctx+0x1DA8, the per-view cloud state
//     v20 = v5[8] == 1;                  // an int at cloudState+32
//     if (v20) { shader -661749514 } else { shader -298149306 }
// One int chooses between two different lighting shaders. If the views disagree on it they are
// lighting their clouds with different permutations, which is exactly a brightness difference --
// and it fits the audit, where this node costs the second view 2.2x what it costs MAIN, and the
// clouds node 1.7x. More time means the heavier branch.
//
// Everything upstream is already equal by measurement: viewData has no atmosphere field left
// differing, the cloud CB's only mirrorable field (0x40) is mirrored, and the pass's other inputs
// viewData+0x430 / +0x550 sit inside mirrored ranges. This selector is what is left.
//
// Probe first, write second. CloudLightMirror defaults OFF: swapping a shader permutation is not
// the same class of change as copying a float, and the measurement costs nothing.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CloudLightProbe  = 1;   // answered: selector 2 on both; the state object is pooled per frame and not diffable across views
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CloudLightMirror = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugCloudSelMain  = 0xFFFFFFFF;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugCloudSelVrcam = 0xFFFFFFFF;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCloudSelWrites = 0;

// RULED OUT: the selector reads 2 on both views, so they light the clouds with the same shader
// (-298149306, the else branch). Kept as a live probe because it cost nothing and the answer is
// worth keeping visible.
//
// That leaves the cloud state object itself. ctx+0x1DA8 is the one structure in this whole chase
// that has never been diffed -- viewData, the graph context and the cloud constant buffer all
// have been, and each of the three named its own defect. Same instrument, last structure. The
// known fields are +24/+28 (resources), +32 (the selector) and +40 (read by the clouds node), so
// 256 bytes with the chunked copy covers it without assuming a size.
static uint8_t g_cloudst_main[256];
static std::atomic<size_t> g_cloudst_len{0};
static std::mutex g_cloudst_mtx;

// SEH cannot share a frame with anything that unwinds (C2712), so the guarded reads live here.
static size_t cloudst_read(void* dst, uintptr_t src, size_t n) {
    size_t done = 0;
    while (done < n) {
        const size_t step = (n - done) < 32 ? (n - done) : 32;
        __try { memcpy(static_cast<uint8_t*>(dst) + done,
                       reinterpret_cast<const void*>(src + done), step); }
        __except (EXCEPTION_EXECUTE_HANDLER) { break; }
        done += step;
    }
    return done;
}
static uintptr_t cloudst_ptr(uintptr_t ctx) {
    __try { return *reinterpret_cast<uintptr_t*>(ctx + 0x2BD8); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return 0; }
}


// ---- the same snapshot, taken at the SAME point of the frame -------------------------------
//
// The dispatch-time version of this was worthless and said so: it snapped MAIN on whatever MAIN
// node ran and VRCAM on whatever VRCAM node ran, i.e. at two unrelated points of the frame. For
// fields the engine fills and clears as the frame proceeds that compares nothing -- which is why
// one capture showed the populated sub-block only on MAIN and the next showed it only on VRCAM,
// and why the counter at +0x18 went DOWN between captures (the object comes from a pool).
//
// Hooking the clouds node instead puts both snapshots at one place: the node's own entry. Then a
// field that still differs really differs.
using CloudsNodeFn = char(__fastcall*)(void*, void*);
CloudsNodeFn g_orig_clouds_node = nullptr;
static uint8_t g_cst_snap[2][256];
static size_t  g_cst_len[2] = { 0, 0 };
static std::mutex g_cst_mtx;

static void clouds_node_note(uintptr_t ctx, bool vrcam) {
    const uintptr_t st = cloudst_ptr(ctx);
    if (!st) return;
    uint8_t cur[256];
    const size_t got = cloudst_read(cur, st, sizeof(cur));
    if (got < 64) return;
    bool report = false;
    uint8_t ref[256];
    size_t n = 0;
    {
        std::lock_guard<std::mutex> lk(g_cst_mtx);
        memcpy(g_cst_snap[vrcam ? 1 : 0], cur, got);
        g_cst_len[vrcam ? 1 : 0] = got;
        if (vrcam && g_cst_len[0]) {
            n = g_cst_len[0] < got ? g_cst_len[0] : got;
            n &= ~size_t(3);
            memcpy(ref, g_cst_snap[0], n);
            report = n >= 64;
        }
    }
    if (!report) return;
    static std::atomic<uint64_t> s_next{0};
    const uint64_t now = GetTickCount64();
    uint64_t due = s_next.load(std::memory_order_relaxed);
    if (now < due) return;
    if (!s_next.compare_exchange_strong(due, now + 6000, std::memory_order_relaxed)) return;
    const uint32_t* m = reinterpret_cast<const uint32_t*>(ref);
    const uint32_t* v = reinterpret_cast<const uint32_t*>(cur);
    char line[900];
    int used = 0;
    line[0] = 0;
    for (size_t k = 0; k < n / 4; ++k) {
        if (m[k] == v[k]) continue;
        if (used < static_cast<int>(sizeof(line)) - 32)
            used += snprintf(line + used, sizeof(line) - used, "%zX{%08X|%08X} ",
                             k * 4, m[k], v[k]);
    }
    log("[cloudnode] state at the clouds-node entry, %zu bytes, M|V: %s",
        n, used ? line : "(identical)");
}

static uintptr_t clouds_ctx_of(void* wc) {
    __try { return wc ? *reinterpret_cast<uintptr_t*>(
                            reinterpret_cast<uint8_t*>(wc) + 0x18) : 0; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return 0; }
}
static bool clouds_is_vrcam(uintptr_t ctx) {
    __try { return ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

char __fastcall Detour_CloudsNode(void* a1, void* a2) {
    if (CyberpunkVR_CloudLightProbe) {
        const uintptr_t ctx = clouds_ctx_of(a2);
        if (ctx) clouds_node_note(ctx, clouds_is_vrcam(ctx));
    }
    return g_orig_clouds_node(a1, a2);
}


static void cloud_sel_note(uintptr_t ctx, bool vrcam) {
    if (!CyberpunkVR_CloudLightProbe || !ctx) return;
    __try {
        const uintptr_t st = *reinterpret_cast<uintptr_t*>(ctx + 0x2BD8);
        if (!st) return;
        int32_t* sel = reinterpret_cast<int32_t*>(st + 32);
        if (vrcam) {
            CyberpunkVR_DebugCloudSelVrcam = static_cast<uint32_t>(*sel);
            if (CyberpunkVR_CloudLightMirror &&
                CyberpunkVR_DebugCloudSelMain != 0xFFFFFFFF &&
                *sel != static_cast<int32_t>(CyberpunkVR_DebugCloudSelMain)) {
                *sel = static_cast<int32_t>(CyberpunkVR_DebugCloudSelMain);
                CVR_DIAGNOSTIC(InterlockedIncrement64(
                    reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCloudSelWrites)));
            }
        } else {
            CyberpunkVR_DebugCloudSelMain = static_cast<uint32_t>(*sel);
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    static std::atomic<uint64_t> s_next{0};
    const uint64_t now = GetTickCount64();
    uint64_t due = s_next.load(std::memory_order_relaxed);
    if (now < due) return;
    if (!s_next.compare_exchange_strong(due, now + 5000, std::memory_order_relaxed)) return;
    log("[cloudsel] cloudState+32 (picks the lighting shader) -- MAIN %d  VRCAM %d  %s  "
        "mirror=%d writes=%llu",
        (int)CyberpunkVR_DebugCloudSelMain, (int)CyberpunkVR_DebugCloudSelVrcam,
        (CyberpunkVR_DebugCloudSelMain == CyberpunkVR_DebugCloudSelVrcam)
            ? "same shader" : "<- DIFFERENT SHADERS",
        CyberpunkVR_CloudLightMirror, CyberpunkVR_DebugCloudSelWrites);
}


static void sky_probe_note(uintptr_t ctx, bool vrcam) {
    if (!CyberpunkVR_SkyProbe || !ctx) return;
    __try {
        const uint32_t idx = *reinterpret_cast<uint8_t*>(ctx + 0x24C8);
        if (vrcam) CyberpunkVR_DebugSkyIdxVrcam = idx;
        else       CyberpunkVR_DebugSkyIdxMain  = idx;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    static std::atomic<uint64_t> s_next{0};
    const uint64_t now = GetTickCount64();
    uint64_t due = s_next.load(std::memory_order_relaxed);
    if (now < due) return;
    if (!s_next.compare_exchange_strong(due, now + 5000, std::memory_order_relaxed)) return;
    log("[sky] sky-record index view+0x24C8 -- MAIN %u  VRCAM %u   %s     "
        "AA mode viewData+0xFA4 -- MAIN %u  VRCAM %u",
        CyberpunkVR_DebugSkyIdxMain, CyberpunkVR_DebugSkyIdxVrcam,
        (CyberpunkVR_DebugSkyIdxMain == CyberpunkVR_DebugSkyIdxVrcam)
            ? "<- SAME RECORD: the two views share one amortised sky"
            : "<- separate records",
        CyberpunkVR_DebugMainAaMode, CyberpunkVR_DebugVrcamAaMode);
}

extern "C" __declspec(dllexport) int32_t CyberpunkVR_OpticsRenderProbe=0;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_OpticsFreezeCopy=0;
static void optics_render_probe(uintptr_t ctx,void* work,uint64_t key,uint32_t rva) {
    if(!CyberpunkVR_OpticsRenderProbe || !ctx || !work)return;
    const int side=key==0?0:key==g_vrcam_ctx_key?1:key==cvr::cname_hash("vr_optics_feed")?2:-1;
    if(side<0)return;
    constexpr uint32_t nodes[]={0x557204,0xf60474,0x3fe078,0xdac960,0x411b94,0x3e1b20,0x7f3c20};
    uint32_t bit=0;
    for(uint32_t i=0;i<std::size(nodes);++i)if(rva==nodes[i])bit=1u<<i;
    if(!bit)return;
    static std::atomic<uint32_t> seen[3]{};
    if(seen[side].fetch_or(bit)&bit)return;
    __try {
        const auto mgr=*reinterpret_cast<const uintptr_t*>(ctx+0x2B90);
        log("[optics-render] side=%d node=%X flags=%016llX/%016llX execution=%u selector102=%u",
            side,rva,*reinterpret_cast<const uint64_t*>(ctx+0x25B0),
            *reinterpret_cast<const uint64_t*>(ctx+0x25B8),
            static_cast<const uint8_t*>(work)[48],mgr?*reinterpret_cast<const uint32_t*>(mgr+408):0);
    } __except(EXCEPTION_EXECUTE_HANDLER) {}
}

uint8_t __fastcall Detour_NodeDispatch(
        uintptr_t* node, uint8_t* work_context, void* args) {
    // Attribution FIRST (work-fn, vrcam view, SceneDrv rtId): used by the profiler,
    // the NODE-CUT gate and the mirror path. The OM/barrier hooks read
    // t_current_node_work to gate the tonemap RT0 snapshot + mirror RTV capture.
    bool vrcam_node = false;
    bool node_owner_bit = false;  // Legacy profiler name: +0x30 bit1 gates actual
                                  // DoCulling execution, not shared-work ownership.
    uintptr_t prof_work = 0;
    uint8_t scene_rtid = 0xFF;    // pass/RT slot id from ctx+0x38
    uint64_t view_key = 0;        // ctx+0x28: 0 = MAIN, g_vrcam_ctx_key = VRCAM, else other
    bool view_key_known = false;  // false when this node carries no view ctx at all
    uintptr_t poseContext=0;
    __try {
        const uintptr_t vtable = node ? *node : 0;
        prof_work = vtable ? *reinterpret_cast<uintptr_t*>(vtable + 8) : 0;
        if (work_context) {
            const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(work_context + 0x18);
            if (ctx) {
                view_key = *reinterpret_cast<uint64_t*>(ctx + 0x28);
                poseContext=ctx;
                view_key_known = true;
            }
            if (ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key) {
                vrcam_node = true;   // g_vrcam_ctx_key
                // Belt and braces: fg_observe grants this at graph-build time, but only for the
                // builder path it sees. Re-asserting it here means no node of this view can run
                // before the capability is present, whichever of them collects the HUD.
                hud_grant_capability(ctx);
                // Named render-mask categories (DistantLights by default). Same mechanism as
                // the HUD grant above, but sourced from the engine's own descriptor rather
                // than from hand-picked bit numbers.
                render_mask_grant(ctx);
            }
            if (ctx) sky_probe_note(ctx, view_key == g_vrcam_ctx_key);
            if (ctx) cloud_sel_note(ctx, view_key == g_vrcam_ctx_key);
            // Always-on chain diagnostic. Zero here means the engine never dispatched a node
            // for a view whose key matches ours -- i.e. the component is not enabled, or its
            // virtualCameraName is not what we hashed. Non-zero here with a dead mirror moves
            // the search downstream (RTV capture / blit submit).
            if (vrcam_node) { CVR_DIAGNOSTIC(++CyberpunkVR_DebugVrcamNodeHits); render_mask_report(); }
            // MAIN identity, step 1. Note the deliberate absence of a `ctx` requirement:
            // these nodes run with work_context+0x18 == 0, so a ctx-keyed bind here can never
            // fire. The view OBJECT is what they carry, so that is what we record.
            if (!vrcam_node && (!view_key_known || view_key==0) && prof_work && g_exe_base) {
                const uint32_t rva = (uint32_t)(prof_work - (uintptr_t)g_exe_base);
                if (rva == MAIN_PRESENT_WORK_RVA || rva == MAIN_STARTRENDER_WORK_RVA) {
                    const uintptr_t obj = sl_view_obj(work_context);
                    if (obj &&
                        g_main_view_obj.exchange(obj, std::memory_order_release) != obj)
                        CVR_DIAGNOSTIC(++CyberpunkVR_DebugMainObjBinds);
                }
            }
            node_owner_bit = (work_context[0x30] & 2) != 0;
            // rtId is ctx+0x38. There is NO valid-flag at +0x39: the engine's own executor
            // indexes its RT table with *(u8*)(a2+56) unconditionally (engine_re/dumps/
            // B_framegraph.md:7,45). The old `ctx[0x39]==1` guard was a bad inference and
            // dropped ~90% of the scene passes (4 of 41 SceneDrv calls/frame got attributed).
            scene_rtid = work_context[0x38];
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) { prof_work = 0; }
    // Save/restore, like t_vrcam_node_active below. Without the restore a node that dispatches
    // child nodes has this pointing at the LAST CHILD for the rest of its body, so every GPU
    // call it makes afterwards is attributed to the child. That is not academic: it is what put
    // ClusteredLightsCull's 1964-group dispatch under one node for MAIN and another for VRCAM,
    // making a shared dispatch look MAIN-exclusive in the census.
    const uintptr_t previous_node_work = t_current_node_work;
    const uintptr_t previous_node_view = t_current_node_view;
    t_current_node_work = prof_work;
    t_current_node_view = poseContext;
    const uint32_t work_rva = (prof_work && g_exe_base &&
            prof_work > reinterpret_cast<uintptr_t>(g_exe_base))
        ? static_cast<uint32_t>(prof_work - reinterpret_cast<uintptr_t>(g_exe_base)) : 0;
    // Close VRCAM's viewData holes as early as the view is seen at all, so every consumer in
    // the frame reads the filled value rather than the zero the pool handed out.
    if ((CyberpunkVR_ViewDataFixMask || CyberpunkVR_FogMirrorMask ||
         CyberpunkVR_EnvMirrorMask || CyberpunkVR_CompLendSet) && vrcam_node && work_context)
        viewdata_fill_from_wc(work_context);
    if(view_key_known && view_key==optics_view_key() && work_context) {
        LARGE_INTEGER p0{};
        if(CyberpunkVR_MaskShareTiming)QueryPerformanceCounter(&p0);
        render_mask_grant(poseContext,false);
        optics_hide_weapon_plane(poseContext);
        viewdata_fill_from_wc(work_context,true);
        if(CyberpunkVR_MaskShareTiming) {
            LARGE_INTEGER p1{};
            QueryPerformanceCounter(&p1);
            InterlockedAdd64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_OpticsPreworkTicks),p1.QuadPart-p0.QuadPart);
            InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_OpticsPreworkCalls));
        }
    }
    // Per-view node census. Only views we can name: nodes dispatched with no view ctx are
    // global and cannot be attributed to either eye.
    if (CyberpunkVR_NodeCensus && work_rva && view_key_known) {
        if (view_key == 0) { node_census_add(work_rva, false); node_census_dump(); }
        else if (view_key == g_vrcam_ctx_key) node_census_add(work_rva, true);
    }
    // NODE CUT census: skip the whole node when an armed rule matches (see table above).
    if (CyberpunkVR_NodeCutEnable && work_rva &&
            node_cut_match(work_rva, scene_rtid, vrcam_node)) {
        CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
            &CyberpunkVR_DebugNodeCutSkips)));
        t_current_node_work = previous_node_work;
        t_current_node_view = previous_node_view;
        return static_cast<uint8_t>(CyberpunkVR_NodeCutRetVal);
    }
    // Mirror: detect the vrcam CopyToTexture node and arm per-node capture state.
    const bool mirror_copy_node = is_vrcam_copy_to_texture(node, work_context);
    const bool previous_mirror_active = t_mirror_copy_node_active;
    ID3D12Resource* const previous_mirror_rtv = t_mirror_copy_rtv;
    const DXGI_FORMAT previous_mirror_format = t_mirror_copy_rtv_format;
    ID3D12GraphicsCommandList* const previous_mirror_list = t_mirror_copy_list;
    if (mirror_copy_node) {
        CVR_DIAGNOSTIC(g_eye_node_hits.fetch_add(1, std::memory_order_relaxed));
        t_mirror_copy_node_active = true;
        t_mirror_copy_rtv = nullptr;
        t_mirror_copy_rtv_format = DXGI_FORMAT_UNKNOWN;
        t_mirror_copy_list = nullptr;
        t_mirror_src_state = (uint32_t)D3D12_RESOURCE_STATE_RENDER_TARGET;
        t_mirror_src_state_seen = false;   // a seed, not an observation
    }
    // Does the HUD node reach the second view at all? This is the measurement that decides
    // whether the capability override can work: an override is useless on a node the engine
    // never dispatches for that view. Counted unconditionally -- two compares on a path that
    // already computed work_rva.
    if (work_rva == DRAWHUD_WORK_RVA) {
        if (vrcam_node) CVR_DIAGNOSTIC(InterlockedIncrement64(
                            reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugHudNodeVrcam)));
        else            CVR_DIAGNOSTIC(InterlockedIncrement64(
                            reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugHudNodeMain)));
        // The list of bit positions the second view lacks -- once per view, then never again.
        hud_dump_capability_mask(work_context, vrcam_node);
        // Readable without a debugger: one line every ~15 s, keyed off the MAIN count so it
        // cannot spin when the second view is absent.
        if (cvr::RuntimeDiagnosticsEnabled() && (CyberpunkVR_DebugHudNodeMain % 900) == 1) {
            log("[hud] DrawHUD main=%llu vrcam=%llu | gate denied=%llu forced=%llu | "
                "blocks null=%llu ok=%llu lent=%llu | capGrants=%llu w%u=%016llX "
                "| HudInVrcam=%d borrow=%d grant=%d",
                (unsigned long long)CyberpunkVR_DebugHudNodeMain,
                (unsigned long long)CyberpunkVR_DebugHudNodeVrcam,
                (unsigned long long)CyberpunkVR_DebugHudGateDenied,
                (unsigned long long)CyberpunkVR_DebugHudGateForced,
                (unsigned long long)CyberpunkVR_DebugHudBlockNull,
                (unsigned long long)CyberpunkVR_DebugHudBlockOk,
                (unsigned long long)CyberpunkVR_DebugHudBlockLent,
                (unsigned long long)CyberpunkVR_DebugHudCapGrants,
                CyberpunkVR_HudCapWord,
                (unsigned long long)CyberpunkVR_HudCapBits,
                CyberpunkVR_HudInVrcam, CyberpunkVR_HudBorrowBlocks,
                CyberpunkVR_HudGrantCap);
        }
    }

    // Lend MAIN's draw-block list to the second eye for the duration of the HUD node.
    // See g_hud_block_main: this is the exit the node actually takes, and the slot is restored
    // below whatever the node does.
    void** hud_block_slot = nullptr;
    void*  hud_block_saved = nullptr;
    if (work_rva == DRAWHUD_WORK_RVA && work_context) {
        if (!g_hud_viewdata_get && g_exe_base) {
            g_hud_viewdata_get = reinterpret_cast<HudViewDataFn>(g_exe_base + 0x33C248);
        }
        if (g_hud_viewdata_get) {
            __try {
                uint8_t* vd = reinterpret_cast<uint8_t*>(g_hud_viewdata_get(work_context));
                if (vd) {
                    void** slot = reinterpret_cast<void**>(vd + 0x168);
                    void* cur = *slot;
                    if (!vrcam_node) {
                        // MAIN's own HUD node, same frame, same node: the most honest source
                        // for the list the second eye is missing.
                        if (cur) g_hud_block_main = cur;
                    } else if (!cur) {
                        CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
                            &CyberpunkVR_DebugHudBlockNull)));
                        if (CyberpunkVR_HudInVrcam && CyberpunkVR_HudBorrowBlocks &&
                            g_hud_block_main) {
                            hud_block_slot = slot;
                            hud_block_saved = cur;
                            *slot = g_hud_block_main;
                            CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
                                &CyberpunkVR_DebugHudBlockLent)));
                        }
                    } else {
                        // Not empty -- then this is NOT where the node stops, and the borrow is
                        // the wrong fix. Counted so that shows up instead of being assumed.
                        CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
                            &CyberpunkVR_DebugHudBlockOk)));
                    }
                }
            } __except (EXCEPTION_EXECUTE_HANDLER) { hud_block_slot = nullptr; }
        }
    }

    const bool previous_vrcam_node = t_vrcam_node_active;
    t_vrcam_node_active = vrcam_node;
    // WHICH OF THE THREE, not "vrcam or else". `t_vrcam_node_active` answers a yes/no question, and every
    // census in this port has been reading its NO as "MAIN" -- which is wrong, because the frame also renders
    // views that are neither eye: reflection-probe cubemap faces go through the same nodes with their own view
    // context. Their constants then landed in the MAIN column, and that is what produced a fog block "differing
    // in 49 of 96 floats" with an axis-aligned basis and a near-origin camera on one side, a sky block whose
    // two "views" disagreed by swapping +-1 between matrix rows, and a 448-byte block reading 1e33. Five
    // separate readings today were mixtures for this one reason.
    //
    // So the view is published as a side: 0 = MAIN, 1 = the second eye, -1 = neither or unknown. A census that
    // ignores -1 compares two eyes and nothing else. Saved and restored like the flag above, because SceneDrv
    // re-enters the dispatcher per pass and a nested node must not leave the parent mis-tagged.
    // AND THE KEY ALONE CANNOT DO IT, which the first attempt at this proved: the value at ctx+0x28 is the
    // CName hash of a virtual camera name, and only the VRCAM component has one. MAIN reads 0 -- and so does
    // every other engine view, reflection-probe faces included. Keying on it therefore still put probe faces
    // in MAIN's column, and the sky block went on reporting two "views" that swapped +-1 between matrix rows.
    //
    // The port already knows MAIN by identity, not by name: g_main_view_obj is bound from the view object the
    // Present and StartRender nodes carry. So MAIN is the dispatch whose view object IS that one, the second
    // eye is the VRCAM key, and everything else is neither.
    const int32_t previous_view_side = t_view_side;
    if (view_key_known && view_key == g_vrcam_ctx_key) {
        t_view_side = 1;
    } else if(view_key_known && view_key!=0) {
        // A named auxiliary camera is neither eye, even if a shared native
        // view-object accessor aliases MAIN's wrapper. It must not publish
        // its descriptors, colour constants or images as MAIN.
        t_view_side = -1;
    } else {
        const uintptr_t main_obj = g_main_view_obj.load(std::memory_order_acquire);
        uintptr_t obj = 0;
        __try { obj = work_context ? sl_view_obj(work_context) : 0; }
        __except (EXCEPTION_EXECUTE_HANDLER) { obj = 0; }
        t_view_side = (main_obj && obj == main_obj) ? 0 : -1;
    }
    if(t_view_side>=0)cvr::optics::source::ViewDispatched(t_view_side,work_context);
    // Publish the exact view for the camera hooks that run inside this dispatch. Saved and
    // restored like the vrcam flag: SceneDrv re-enters the dispatcher per pass, so a nested
    // node must not leave the parent's view mis-tagged.
    const bool     previous_view_known = t_active_view_known;
    const uint64_t previous_view_key   = t_active_view_key;
    t_active_view_known = view_key_known;
    t_active_view_key   = view_key;
    cvr::camera::PoseIdentity nodePose{};
    if(view_key_known && (view_key==0 || view_key==g_vrcam_ctx_key.load(std::memory_order_relaxed)))
        cvr::camera::ReadCameraPoseIdentity(poseContext+cvr::view_layout::kCamera,view_key==0 ? 1u : 2u,&nodePose);
    if(cvr::framegen::CaptureInputsEnabled() && nodePose)
        nodePose.hasRenderFrameId=cvr::framegen::ReadRenderFrameIndex(&nodePose.renderFrameId);
    const auto previousNodePose=cvr::camera::SetNodePoseIdentity(nodePose);
    // Framegen without Streamline: the camera of the view a TAA or upscaler node is about to render.
    if(cvr::framegen::CaptureInputsEnabled() && nodePose)cvr::framegen::ObserveNativeView(work_context);
    const bool fogNode=work_rva==VOLUMETRIC_FOG_NODE_RVA;
    const auto previousFogContext=fogNode ? cvr::stereo::SetFogWorkContext(reinterpret_cast<uintptr_t>(work_context)):0;

    // Profile EVERY depth, not just the outermost. SceneDrv (+0x332D5C) drives ~37 scene
    // passes back through this same hook, so a depth==0 guard gives those child nodes ZERO
    // rows and buries their cost inside the parent -- which is why RenderElements & co were
    // invisible in the profiler and only showed up in the (now dead) array harness.
    // We therefore time all depths and record BOTH inclusive and self time, using a
    // thread-local accumulator through which each node reports its inclusive time upward.
    int64_t prof_t0 = 0, prof_saved_child = 0;
    const bool prof_on = CyberpunkVR_ProfEnable != 0;
    const bool prof_top = prof_on && t_prof_disp_depth == 0;
    // Timeline 1 = top-level nodes; 2 adds the nested ones (kind 16), so a view's scene passes
    // can be told apart by what runs inside them.
    const uint32_t timeline_depth = static_cast<uint32_t>(t_prof_disp_depth);
    const bool timeline = CyberpunkVR_NodeTimeline != 0 && (timeline_depth == 0 || CyberpunkVR_NodeTimeline >= 2);
    const int64_t timeline_t0 = timeline ? prof_now() : 0;
    ++t_prof_disp_depth;
    if (prof_on) {
        prof_saved_child = t_prof_child_ticks;
        t_prof_child_ticks = 0;
        prof_t0 = prof_now();
    }
    // DrawHUD reads viewData+0x168 too (measured); EngineRvas has no name for it, so one here.
    constexpr uint32_t DRAWHUD_NODE_RVA_168 = 0x33E388;
    // The composition state, for the measured readers of viewData+0x168. NOT for
    // DrawComposition (0x33D0AC): it walks a list of composition elements built for another view
    // and crashes on it -- see the note by CyberpunkVR_CompLendScoped.
    uint64_t comp_saved[2] = {0, 0};
    uintptr_t comp_lent_vd = 0;
    if (CyberpunkVR_CompLendScoped && vrcam_node && work_context && prof_work && g_exe_base) {
        const uintptr_t cbase = reinterpret_cast<uintptr_t>(g_exe_base);
        const uint32_t crva = (prof_work > cbase)
                                  ? static_cast<uint32_t>(prof_work - cbase) : 0;
        const bool wanted =
            ((CyberpunkVR_CompLendScoped & 1u) && crva == COMPOSITION_WORK_RVA) ||
            ((CyberpunkVR_CompLendScoped & 2u) && crva == RT_DECLARE_WORK_RVA) ||
            ((CyberpunkVR_CompLendScoped & 4u) && crva == DRAWHUD_NODE_RVA_168) ||
            ((CyberpunkVR_CompLendScoped & 8u) && crva == GATE_READER_770268_RVA);
        if (wanted) comp_lent_vd = comp_lend_install(work_context, comp_saved);
    }
    view_params_report();
    complend_report();
    if(view_key_known)optics_render_probe(poseContext,work_context,view_key,work_rva);
    // Diagnostic only: hold the existing external camera image while the
    // camera, UI and both eye renderers continue normally. Toggling the camera
    // off clears its dynamic texture and cannot isolate sampling artefacts.
    const bool opticalCopy=view_key_known && view_key==cvr::cname_hash("vr_optics_feed") && work_rva==0x3FE078;
    const bool freezeOpticalImage=CyberpunkVR_OpticsFreezeCopy && opticalCopy;
    // The camera image the scope displays is written here; the port accumulates it right after
    // this node's list runs (optics::camera).
    if(opticalCopy && !freezeOpticalImage)cvr::optics::camera::CopyNodeBegin(work_context);
    const uint8_t result = freezeOpticalImage ? 1 :
        cvr::stereo::preparation::InvokeGraphContext(g_node_dispatch_orig,node,work_context,args);
    if(opticalCopy && !freezeOpticalImage)cvr::optics::camera::CopyNodeEnd();
    if(view_key_known && cvr::optics::probe::StageView(view_key,g_vrcam_ctx_key) && work_rva==0x416600)
        cvr::optics::probe::StageEnd(g_game_device);
    if (comp_lent_vd) comp_lend_restore(comp_lent_vd, comp_saved);
    // Give the slot back before anything else can run on this view.
    if (hud_block_slot) {
        __try { *hud_block_slot = hud_block_saved; }
        __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    if (prof_on) {
        const int64_t dt = prof_now() - prof_t0;
        int64_t self = dt - t_prof_child_ticks;
        if (self < 0) self = 0;                       // clock jitter across cores
        t_prof_child_ticks = prof_saved_child + dt;   // hand our inclusive time to the parent
        if (prof_top) {   // frame totals stay top-level only, else they double-count
            if (vrcam_node) {
                g_prof_disp_vrcam_ticks.fetch_add(dt, std::memory_order_relaxed);
                g_prof_disp_vrcam_nodes.fetch_add(1, std::memory_order_relaxed);
                g_prof_top_vrcam.fetch_add(1, std::memory_order_relaxed);
            } else {
                g_prof_disp_main_ticks.fetch_add(dt, std::memory_order_relaxed);
                g_prof_disp_main_nodes.fetch_add(1, std::memory_order_relaxed);
                g_prof_top_main.fetch_add(1, std::memory_order_relaxed);
            }
        }
        prof_node_add(prof_work, dt, self, vrcam_node);
        prof_pair_add(scene_rtid, work_rva, self, vrcam_node, !prof_top, node_owner_bit);
        if (work_rva == SCENE_DRIVER_WORK_RVA) {
            ProfPass& p = g_prof_scenepass[scene_rtid];
            if (vrcam_node) {
                p.ticks_vrcam.fetch_add(dt, std::memory_order_relaxed);
                p.calls_vrcam.fetch_add(1, std::memory_order_relaxed);
            } else {
                p.ticks_main.fetch_add(dt, std::memory_order_relaxed);
                p.calls_main.fetch_add(1, std::memory_order_relaxed);
            }
        }
    }
    --t_prof_disp_depth;
    if (timeline) {
        // extra: bit 8 set = a scene pass, bits 0-7 its rtId; nested spans add the side (bits 12-13)
        // and the depth (bits 16-19).
        const uint32_t side = vrcam_node ? 1u : (view_key_known && view_key != 0) ? 2u : 0u;
        const uint32_t pass = 0x100u | scene_rtid;
        if (timeline_depth == 0) prof_timeline_add(timeline_t0, prof_now(), work_rva, side, pass);
        else prof_timeline_add(timeline_t0, prof_now(), work_rva, 16u,
                               pass | side << 12 | (timeline_depth < 15u ? timeline_depth : 15u) << 16);
    }

    const int32_t nodeSide=t_view_side;
    t_vrcam_node_active = previous_vrcam_node;
    t_view_side = previous_view_side;
    t_active_view_known = previous_view_known;
    t_active_view_key   = previous_view_key;
    cvr::camera::SetNodePoseIdentity(previousNodePose);
    if(fogNode)cvr::stereo::SetFogWorkContext(previousFogContext);
    t_current_node_work = previous_node_work;
    t_current_node_view = previous_node_view;
    const bool traceCopy=cvr::stereo::preparation::Pairing();
    cvr::stereo::preparation::CopySource previousCopy{};
    if(traceCopy)previousCopy=cvr::stereo::preparation::SetCopySource({poseContext,work_rva,nodeSide});
    // THE FINISHED FRAME, at the epilogue of the second-view node that produced it. Same window as
    // every other snapshot taken here, and for the same reasons: the work-fn has returned so both
    // indirect writes are recorded, the list is still open, and the release barrier that aliases the
    // heap away arrives later, in a batch at the tail of the list.
    //
    // Gated on vrcam_node alone, deliberately. The barrier that arms this sits between the Discard and
    // the writes inside one node's work-fn, so the first second-view epilogue to run is that node's
    // own -- and scoping it to a node RVA is exactly what made the barrier-side copy miss four frames
    // in five.
    if (vrcam_node) final_frame_copy_now();
    // Tonemap OUTPUT snapshot -> our committed g_stable_tex (flicker fix). Fires at the
    // tonemap node's own epilogue while its list is still open and RT0 not yet aliased.
    // EACH SOURCE'S OWN GATE, ON ITS OWN RESOURCE -- t_tm_from_final says who armed the slot. Opening
    // this for one source alone let it fire on the OTHER source's target, and that single copy latched
    // g_have_tonemap_source, which stood the second writer down and suppressed the mirror path too:
    // nothing was left feeding the eye and the session ended in DXGI_ERROR_INVALID_CALL.
    if (((CyberpunkVR_StableFromTonemap && !t_tm_from_final) ||
             (CyberpunkVR_FinalGrab && t_tm_from_final)) &&
            !t_tm_consumed && t_tm_rt0 && t_tm_rt0_list &&
            CyberpunkVR_StableCopy && stereo_eye_capture_wanted() &&
            (t_tm_rt0_state_seen || !CyberpunkVR_NoStateLies)) {
        mirror_stable_inline_copy(t_tm_rt0_list, t_tm_rt0, t_tm_rt0_state,&nodePose);
        t_tm_consumed = true;
        // AND RELEASE THE SLOT. Marking it consumed is not enough once the armed resource is a
        // per-frame TRANSIENT out of a pool whose heap is recycled after the frame, so a pointer left
        // behind here outlives its resource and any later touch of it is invalid. The barrier learns it
        // again next frame, so clearing costs nothing. 0.1.6 used a 0xFFFFFFFF state sentinel for this;
        // this branch already carries t_tm_rt0_state_seen, so that is what is cleared instead.
        t_tm_rt0 = nullptr;
        t_tm_rt0_list = nullptr;
        t_tm_rt0_state_seen = false;
        // Only a copy from the PREFERRED source may claim the role; otherwise the mirror path gets
        // suppressed by a copy that was never the intended image.
        if (t_tm_from_final || CyberpunkVR_StableFromTonemap)
            g_have_tonemap_source.store(true, std::memory_order_release);
        CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
            &CyberpunkVR_DebugTonemapSnaps)));
    }
    ID3D12Resource* const mirror_output = mirror_copy_node ? t_mirror_copy_rtv : nullptr;
    const DXGI_FORMAT mirror_output_format = mirror_copy_node ? t_mirror_copy_rtv_format
                                                             : DXGI_FORMAT_UNKNOWN;
    ID3D12GraphicsCommandList* const mirror_list = mirror_copy_node ? t_mirror_copy_list : nullptr;
    const uint32_t mirror_src_state = t_mirror_src_state;
    if (mirror_copy_node) {
        t_mirror_copy_node_active = previous_mirror_active;
        t_mirror_copy_rtv = previous_mirror_rtv;
        t_mirror_copy_rtv_format = previous_mirror_format;
        t_mirror_copy_list = previous_mirror_list;
    }
    if (mirror_output) {
        // Valid-window snapshot: the node work-fn just returned, so the final write is
        // recorded on mirror_list and no later pass aliased it yet. Final2D is the
        // flapping source -> skip it only when a tonemap snapshot is actually available.
        // AND IT MUST NAME EVERY PREFERRED SOURCE. Leaving xr_final_grab out of this test was the whole
        // reason the finished frame never reached the headset in 0.1.6: the grab landed 1935 times in a
        // session while THIS path went on copying its own source every frame, and the two formats differ
        // (R8G8B8A8_TYPELESS against R11G11B10_FLOAT), so the snapshot was recreated per frame.
        //
        // Conditional on the latch, never on the key alone: in a frame where the preferred source found
        // nothing (menu, loading) this path must still feed the eye, or the submit works on a texture
        // nothing ever wrote and the device goes out with DXGI_ERROR_INVALID_CALL.
        const bool tonemap_src = (CyberpunkVR_StableFromTonemap || CyberpunkVR_FinalGrab) &&
            g_have_tonemap_source.load(std::memory_order_acquire);
        if (CyberpunkVR_StableCopy && stereo_eye_capture_wanted() && mirror_list && !tonemap_src &&
                (t_mirror_src_state_seen || !CyberpunkVR_NoStateLies)) {
            CVR_DIAGNOSTIC(g_eye_copy_calls.fetch_add(1, std::memory_order_relaxed));
            mirror_stable_inline_copy(mirror_list, mirror_output, mirror_src_state,&nodePose);
        } else if (CyberpunkVR_NoStateLies && !t_mirror_src_state_seen && mirror_list &&
                   CyberpunkVR_StableCopy && stereo_eye_capture_wanted() && !tonemap_src) {
            // Refused rather than guessed. Counted, so this can never become a silent stall.
            CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
                &CyberpunkVR_DebugForeignStateRefusals)));
        } else if (CyberpunkVR_StableCopy && stereo_eye_capture_wanted() && !tonemap_src) {
            // The output target was found but there is no command list to record the copy on.
            // publish() below does not need one, which is exactly why this case can starve the
            // eye while every existing diagnostic reports health.
            CVR_DIAGNOSTIC(g_eye_no_list.fetch_add(1, std::memory_order_relaxed));
        }
        mirror_publish_output(mirror_output, mirror_output_format);
        const uint64_t serial = g_mirror_vrcam_serial.load(std::memory_order_acquire);
        uint64_t armed = g_mirror_armed_serial.load(std::memory_order_relaxed);
        if (serial && armed != serial &&
            g_mirror_armed_serial.compare_exchange_strong(
                armed, serial, std::memory_order_acq_rel)) {
            g_mirror_copy_armed.store(true, std::memory_order_release);
            CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
                &CyberpunkVR_DebugMirrorCopyArms)));
        }
    } else if (mirror_copy_node) {
        // The node ran and bound nothing we recognised as its output. Nothing downstream fires --
        // not the snapshot, not publish -- so this is the one branch that is silent everywhere.
        CVR_DIAGNOSTIC(g_eye_no_rtv.fetch_add(1, std::memory_order_relaxed));
    }
    if(traceCopy)cvr::stereo::preparation::SetCopySource(previousCopy);
    return result;
}

}  // namespace detail
}  // namespace cvr
