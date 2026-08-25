// Hud -- getting the interface into the second eye.
//
// THE FINDING THIS FILE EXISTS BECAUSE OF: the HUD is not drawn into the view's colour target. It has
// its own surface, with PREMULTIPLIED alpha, produced by a node of its own. So the second eye cannot
// get the HUD by copying the first eye's image -- it has to snapshot that surface and composite it.
// Everything here follows from that one fact.
//
// The pieces, in the order they run:
//
//   hud_rt_signature / hud_blur_signature   recognise the surface by its resource description
//   hud_node_note / hud_adopt_by_node       decide which bound target IS the HUD this frame
//   hud_register_rtv                        remember the descriptor that names it
//   hud_snapshot_copy                       take the copy, on a list already being submitted
//   hud_composite_params                    hand the blit what it needs to place and blend it
//   hud_cb_rescan                           find the constant buffer again after a graph rebuild
//
// WHY THE SURFACE IS RE-IDENTIFIED EVERY GRAPH REBUILD rather than cached once: the engine's frame
// graph reallocates its transients, so a pointer that named the HUD last frame can name something
// else after a rebuild. hud_rearm_for_new_graph is what stops the composite from confidently drawing
// the wrong texture -- and drawing the wrong texture is not a crash, it is a wrong picture, which is
// why the identification is by SIGNATURE and re-run rather than by address and remembered.
//
// The counters CyberpunkVR_NoteHudCompositeInputs reports answer "which of the five inputs is the
// composite still waiting for" -- the question that turns "the HUD is missing" into something with
// an address in it.

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

static bool hud_rt_signature(const D3D12_RESOURCE_DESC& d) {
    if (d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D) return false;
    if (d.MipLevels <= 1 || d.DepthOrArraySize != 1 || d.SampleDesc.Count != 1) return false;
    if (!(d.Flags & D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET)) return false;
    if (d.Width < 640 || d.Height < 360) return false;   // never a small ink widget target
    return d.Format == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB ||
           d.Format == DXGI_FORMAT_R8G8B8A8_UNORM ||
           d.Format == DXGI_FORMAT_R8G8B8A8_TYPELESS;
}

// ONE current HUD surface, held by reference, with the handful of mip-0 RTV handles that point
// at it. Deliberately not a growing handle->resource table: the engine recreates the HUD surface
// (graph rebuild, resolution change, enabling the mirror), and a table that keeps the old entries
// would hand a raw, possibly-freed pointer to the copy below, or -- worse -- treat an unrelated
// bind through a recycled descriptor slot as "the HUD is finished" and assert RENDER_TARGET on a
// resource that is nothing of the kind. Registering a new HUD surface therefore RETIRES the old
// one outright, and the resource is AddRef'd for as long as we can name it.
static std::mutex g_hud_rtv_mtx;
ID3D12Resource* g_hud_res = nullptr;          // AddRef'd
static std::array<SIZE_T, 8> g_hud_handles{};
static std::atomic<uint32_t> g_hud_handle_count{0};
// Subresource index of the HUD's last mip -- the snapshot waits for it, because the composite's
// glow samples mips 1, 2 and 4.
std::atomic<uint32_t> g_hud_last_mip{0};
// One-shot: dump the contents of the barrier batch that retires the HUD, so the two
// textures we need are identified from what is actually there rather than guessed.
bool g_hud_batch_listed = false;

// The descriptor signature above is NOT unique, and a log taken in the inventory proves it:
//
//     [hud] surface res=...  1024x1024 mips=11 fmt=27
//
// That is the character-portrait render target -- 1024x1024, a full mip chain, RGBA8 -- and it
// satisfies every clause. It was then adopted as "the HUD" and composited into the second eye,
// which is the yellow full-screen face the menu showed. The signature was validated against one
// gameplay capture ("exactly one such resource in the capture"); menus simply have another one.
//
// So name it the way the outline and the sight were named: by WHO DRAWS INTO IT. rva 0x1EE760 is
// CRenderNode_DrawHUD, the node the RenderMask/HUD descriptor gates, and the render target bound
// while it is on the stack is the HUD surface by construction. Once the node has spoken, the
// descriptor path stops adopting entirely, so no menu resource can take the surface over.
//
// If that node never binds one -- a graph we have not seen -- nothing is named, the flag stays
// false and today's behaviour continues unchanged. This cannot be worse than what it replaces.
// MEASURED, after 0x1EE760 turned out to bind nothing at all. The [hudnode] table, live:
//
//   20A264 : 2560x2560 m5 f29  (M675/V0)     CRenderNode_DrawComposition
//   1F8928 : 2560x2560 m5 f29  (M2696/V0)    CRenderNode_CompositionPostProcess
//
// Those two, and only those two, ever bind a HUD-shaped target -- the same surface each time,
// 2560x2560 with 5 mips in sRGB, exactly the one the capture shows the 2D HUD geometry drawn
// into. 0x1EE760 is where the RenderMask/HUD capability is TESTED, which is not the node body
// that binds; mistaking one for the other is what made the first attempt a silent no-op.
//
// Note what is ABSENT from that table: the inventory's portrait. It is created as an RTV -- which
// is why the descriptor test saw it -- but never bound through this path, so keying on the node
// excludes it by construction rather than by another shape rule.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_HudNodeRva  = 0x1F8928;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_HudNodeRva2 = 0x20A264;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_HudByNode = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudNodeNames = 0;
static std::atomic<bool> g_hud_node_named{false};

extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudRearms = 0;

// OFF. It was a bad trade and the log says so.
//
// Clearing g_hud_node_named turns descriptor matching back on, and that test cannot tell the HUD
// from any other mip-chained RGBA8 render target -- the inventory's character portrait among them,
// as the comment beside the hold has always said. Measured after one inventory close: four
// different surfaces adopted twenty-odd times each, the composite flipping in and out nineteen
// times, and the headset dropping to mono about as long after as CyberpunkVR_HudHoldMs runs.
//
// Before the re-arm the composite died once after a menu and stayed dead: the second eye lost its
// HUD, which is bad. After it, the picture strobes and then loses the second eye entirely, which
// is worse. Stable-and-wrong beats unstable-and-wrong until the actual cause is found, so this
// defaults off and stays switchable for the next attempt at it.
extern "C" __declspec(dllexport) int CyberpunkVR_HudRearmOnGraphChange = 0;

// Called from fg_observe when a FULL frame-graph build shows up under a key never seen before --
// a map or inventory open, and the return from one. Everything the HUD path latched belongs to
// the graph that just went away, so let it be found again in the new one:
//
//   node-named    cleared, which re-enables descriptor matching. This is the one that mattered:
//                 once set it turns matching off, so if the rebuilt graph never re-names the node
//                 there is no second way in and the second eye loses the HUD for the session.
//   held surface  released, so a fresh HUD-shaped target is adopted immediately instead of
//                 waiting out CyberpunkVR_HudHoldMs against a surface the engine already dropped.
//   scan tick     zeroed, so the composite constants are looked for on the next frame rather than
//                 up to two seconds later.
//
// Deliberately NOT cleared: g_hud_cb_from_ring and our own constants copy. The copy is still
// valid memory and still ours; the re-scan refreshes what is in it.
 void hud_rearm_for_new_graph(uint64_t key) {
    if (!CyberpunkVR_HudRearmOnGraphChange) return;
    const bool wasNamed = g_hud_node_named.exchange(false, std::memory_order_acq_rel);
    ID3D12Resource* dropped = nullptr;
    {
        std::lock_guard<std::mutex> lk(g_hud_rtv_mtx);
        dropped = g_hud_res;
        g_hud_res = nullptr;
        g_hud_handle_count.store(0, std::memory_order_release);
    }
    if (dropped) dropped->Release();
    g_hud_cb_scan_tick = 0;

    // GIVE THE NEXT SURFACE A GRACE PERIOD, or the re-arm trades one failure for a worse one.
    //
    // Dropping the incumbent lets a descriptor match adopt again immediately -- and then adopt
    // AGAIN the next frame, because the engine pools these targets: 182 distinct HUD surface
    // pointers in one session. The hold that normally prevents that only bites while the incumbent
    // is demonstrably alive, `GetTickCount64() - g_hud_snap_tick < HudHoldMs`, and a surface
    // adopted one frame ago has not produced a snapshot yet. So the tick was stale, the hold
    // lapsed, the newcomer won, repeat. Measured: sixteen complete/waiting flips in eighty log
    // lines after a map close, which is the second eye's HUD strobing for about a second.
    //
    // Stamping the tick here starts the clock at the re-arm instead of at the first snapshot, so
    // whichever surface is adopted first holds the floor long enough to prove itself. Still
    // self-correcting: if it turns out to be the wrong one it produces nothing, the hold lapses on
    // schedule and the next candidate gets its turn.
    g_hud_snap_tick.store(GetTickCount64(), std::memory_order_release);

    ++CyberpunkVR_DebugHudRearms;
    log("[hud] frame graph rebuilt under key %016llX -- identification re-armed "
        "(was %s, surface %s)", (unsigned long long)key,
        wasNamed ? "node-named" : "unnamed", dropped ? "dropped" : "none held");
}

// The node named in the render-mask table (0x1EE760) never bound a target through this hook --
// zero [hud] surface named by DrawHUD lines in a whole session -- so that rva is the function
// that TESTS the HUD capability, not the node body that binds. Rather than try another guess,
// record which node binds each HUD-shaped target and read the answer off the log.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_HudNodeProbe = 1;   // answered; the two
// binding nodes are hardcoded above. Set to 1 to re-run the survey after an engine update.
struct HudNodeCand { uint32_t rva, w, h, mips, fmt; uint64_t hits[2]; };
static std::array<HudNodeCand, 24> g_hudnode{};
static uint32_t g_hudnode_n = 0;
static std::mutex g_hudnode_mtx;

void hud_node_note(ID3D12Resource* res, bool vrcam) {
    D3D12_RESOURCE_DESC d{};
    if (!mirror_get_resource_desc(res, &d) || !hud_rt_signature(d)) return;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    const uintptr_t work = t_current_node_work;
    const uint32_t rva = (base && work > base) ? static_cast<uint32_t>(work - base) : 0;
    {
        std::lock_guard<std::mutex> lk(g_hudnode_mtx);
        uint32_t i = 0;
        for (; i < g_hudnode_n; ++i)
            if (g_hudnode[i].rva == rva && g_hudnode[i].w == (uint32_t)d.Width &&
                g_hudnode[i].h == d.Height && g_hudnode[i].mips == d.MipLevels) break;
        if (i == g_hudnode_n) {
            if (g_hudnode_n >= g_hudnode.size()) return;
            g_hudnode[g_hudnode_n++] = { rva, (uint32_t)d.Width, d.Height,
                                         d.MipLevels, (uint32_t)d.Format, {0, 0} };
        }
        ++g_hudnode[i].hits[vrcam ? 1 : 0];
    }
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 15000) return;
    s_last = now;
    HudNodeCand c[24]; uint32_t n;
    {
        std::lock_guard<std::mutex> lk(g_hudnode_mtx);
        n = g_hudnode_n;
        for (uint32_t i = 0; i < n; ++i) c[i] = g_hudnode[i];
    }
    char line[900]; int u = 0; line[0] = 0;
    for (uint32_t i = 0; i < n && u < static_cast<int>(sizeof(line)) - 60; ++i)
        u += snprintf(line + u, sizeof(line) - u, "%X:%ux%u m%u f%u(M%llu/V%llu) ",
                      c[i].rva, c[i].w, c[i].h, c[i].mips, c[i].fmt,
                      (unsigned long long)c[i].hits[0], (unsigned long long)c[i].hits[1]);
    log("[hudnode] who binds a HUD-shaped target (%u): %s", n, line);
}

 void hud_adopt_by_node(ID3D12Resource* res) {
    if (!res) return;
    D3D12_RESOURCE_DESC d{};
    if (!mirror_get_resource_desc(res, &d)) return;
    // Node AND shape, not one or the other: the node also binds targets that are not the HUD.
    if (!hud_rt_signature(d)) return;
    std::lock_guard<std::mutex> lk(g_hud_rtv_mtx);
    if (res != g_hud_res) {
        g_hud_handle_count.store(0, std::memory_order_release);
        if (g_hud_res) g_hud_res->Release();
        res->AddRef();
        g_hud_res = res;
        g_hud_last_mip.store(d.MipLevels ? d.MipLevels - 1u : 0u, std::memory_order_release);
        // Throttled, because the engine POOLS these: adoption is not an event, it is a heartbeat.
        // 1877 lines of it in one session -- 42% of the whole log -- and every line said the same
        // thing about a different pointer. The count of swallowed ones rides along, so a genuine
        // storm still shows as a storm.
        LOG_THROTTLED_LC(5000,
            "[hud] surface named by DrawHUD: res=%p %llux%u mips=%u fmt=%u (+%llu more adoptions "
            "since the last line -- the engine pools these)", res,
            (unsigned long long)d.Width, d.Height, (unsigned)d.MipLevels, (unsigned)d.Format,
            (unsigned long long)LOG_SKIPPED);
    }
    if (!g_hud_node_named.exchange(true, std::memory_order_acq_rel))
        log("[hud] identification switched to node 0x%X -- descriptor matching disabled",
            CyberpunkVR_HudNodeRva);
    ++CyberpunkVR_DebugHudNodeNames;
}

// How long a producing surface is protected from being replaced by a descriptor match. The
// descriptor test cannot tell the HUD from the inventory's character portrait -- both are
// mip-chained RGBA8 render targets -- but LIVENESS can: the real HUD is the one that keeps
// yielding snapshots. A newcomer only takes over once the incumbent has gone quiet, which is
// exactly what a genuine graph rebuild or resolution change looks like. Self-correcting too: if
// the wrong surface ever gets in, it produces nothing, the hold lapses and the right one wins.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_HudHoldMs = 1500;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudHolds = 0;

void hud_register_rtv(ID3D12Resource* res,
        const D3D12_RENDER_TARGET_VIEW_DESC* vd, D3D12_CPU_DESCRIPTOR_HANDLE h) {
    if (!res || !h.ptr) return;
    // The node has named it: never let a descriptor match override that.
    if (CyberpunkVR_HudByNode && g_hud_node_named.load(std::memory_order_acquire)) return;
    if (CyberpunkVR_HudHoldMs && res != g_hud_res && g_hud_res) {
        const uint64_t t = g_hud_snap_tick.load(std::memory_order_acquire);
        if (t && GetTickCount64() - t < CyberpunkVR_HudHoldMs) {
            ++CyberpunkVR_DebugHudHolds;
            return;
        }
    }
    // Mip 0 ONLY. The engine also creates RTVs for mips 1..4 to build the HUD glow chain; if
    // those counted as "the HUD target" the snapshot would fire after mip generation, when the
    // subresources are no longer uniformly in RENDER_TARGET and the barrier below would lie.
    if (vd && (vd->ViewDimension != D3D12_RTV_DIMENSION_TEXTURE2D ||
               vd->Texture2D.MipSlice != 0)) {
        return;
    }
    D3D12_RESOURCE_DESC d{};
    const bool is_hud = mirror_get_resource_desc(res, &d) && hud_rt_signature(d);

    std::lock_guard<std::mutex> lk(g_hud_rtv_mtx);
    if (!is_hud) {
        // A descriptor slot we thought was the HUD's, reused for something else -> forget it,
        // rather than let a stale handle trigger a copy from the wrong resource.
        const uint32_t n = g_hud_handle_count.load(std::memory_order_relaxed);
        for (uint32_t i = 0; i < n; ++i) {
            if (g_hud_handles[i] == h.ptr) {
                g_hud_handles[i] = g_hud_handles[n - 1];
                g_hud_handle_count.store(n - 1, std::memory_order_release);
                break;
            }
        }
        return;
    }
    if (res != g_hud_res) {
        g_hud_handle_count.store(0, std::memory_order_release);   // retire the old surface first
        if (g_hud_res) g_hud_res->Release();
        res->AddRef();
        g_hud_res = res;
        g_hud_last_mip.store(d.MipLevels ? d.MipLevels - 1u : 0u, std::memory_order_release);
        log("[hud] surface res=%p %llux%u mips=%u fmt=%u", res,
            (unsigned long long)d.Width, d.Height, (unsigned)d.MipLevels, (unsigned)d.Format);
    }
    uint32_t n = g_hud_handle_count.load(std::memory_order_relaxed);
    for (uint32_t i = 0; i < n; ++i) if (g_hud_handles[i] == h.ptr) return;
    if (n >= g_hud_handles.size()) return;
    g_hud_handles[n] = h.ptr;
    g_hud_handle_count.store(n + 1, std::memory_order_release);
}


// The engine's own blurred-HUD pyramid (half resolution, 4 mips). The composite adds it at lod
// 1.8 with weight _43_m0[5].x -- that is the wide halo on the map, the weapon icons and the
// tracked quest, and it also feeds the shadow term. It is HUD, not scene bloom: the shadow lerps
// ITS alpha against the HUD's, and scene colour has no meaningful alpha. Reproducing it with our
// own mip chain was close but not equal, so we take the engine's.
bool hud_blur_signature(const D3D12_RESOURCE_DESC& d, uint64_t hudWidth) {
    if (d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D) return false;
    if (d.MipLevels != 4 || d.DepthOrArraySize != 1 || d.SampleDesc.Count != 1) return false;
    if (!(d.Flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS)) return false;
    if (d.Format != DXGI_FORMAT_R8G8B8A8_TYPELESS &&
        d.Format != DXGI_FORMAT_R8G8B8A8_UNORM &&
        d.Format != DXGI_FORMAT_R8G8B8A8_UNORM_SRGB) return false;
    if (!hudWidth) return false;
    const uint64_t half = hudWidth / 2;
    return d.Width + 1 >= half && d.Width <= half + 1;
}

std::mutex g_hud_snap_mtx;
// A frame that takes longer than this -- a load spike, a heavy menu -- would blink the HUD off,
// so the window is generous: it only has to be shorter than a menu, not shorter than a hitch.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_HudMaxAgeMs = 1000;
static ID3D12Resource* g_hud_snap = nullptr;
D3D12_RESOURCE_DESC g_hud_snap_desc{};
std::atomic<bool> g_hud_snap_fresh{false};
static ID3D12Resource* g_hud_blur_snap = nullptr;
static D3D12_RESOURCE_DESC g_hud_blur_desc{};
static std::atomic<bool> g_hud_blur_fresh{false};
// MAIN's FINISHED frame and the scene that went under it. With both, the second eye needs no
// reproduction of the HUD at all: the engine's composite is
//     out_main = A + S_main * K          (A = every HUD term, K = how it dims the scene)
// so swapping the scene underneath is exact --
//     out_vrcam = out_main + (S_vrcam - S_main) * K
// and the HUD pixels are literally the engine's, curvature, glow, halo, flicker and all.
// out_main is snapshotted as TYPELESS so it can be read through an _UNORM_SRGB view: the engine
// encodes to sRGB inside the composite, while the scenes are linear.
static ID3D12Resource* g_main_out_snap = nullptr;
static D3D12_RESOURCE_DESC g_main_out_desc{};
static std::atomic<bool> g_main_out_fresh{false};
static ID3D12Resource* g_main_scene_snap = nullptr;
static D3D12_RESOURCE_DESC g_main_scene_desc{};
static std::atomic<bool> g_main_scene_fresh{false};
// The scanner's object outline, as the SECOND eye draws it. Not copied from MAIN: the outline
// traces on-screen silhouettes, so MAIN's would sit at MAIN's parallax and read as double
// vision. VRCAM renders its own -- Resource_83328 at 2444x2560 in EventList_SCANERELEMENTS,
// against MAIN's Resource_85164 -- it is simply never composited, because the chain that turns
// it into a visible outline (PS587 -> PS1047 -> PS1290 -> the PS1216 composite) is MAIN-only,
// behind the same viewData+0x168 gate as the HUD.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_VisionSnap       = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVisionSnaps = 0;
static ID3D12Resource* g_vision_snap = nullptr;
static D3D12_RESOURCE_DESC g_vision_desc{};
static std::atomic<bool> g_vision_fresh{false};
// When the node stops running (view torn down, scanner gone) the last snapshot would otherwise
// keep painting an outline over the second eye for ever. Time-stamped, so it simply expires.
std::atomic<uint64_t> g_vision_tick{0};
// HOW LONG THE LAST OUTLINE STAYS USABLE, and it is the HUD's number and the HUD's escape hatch
// rather than a constant, because the two are the same problem. It was hardcoded at 250 ms, and
// that is what made the outline leave the second eye a moment after each scan: MAIN composites
// from the LIVE surface every frame and does not care whether the compute pass ran again, while
// the second eye works from a snapshot and the pass only runs while something needs redrawing.
// A quarter of a second after the scan settled, our copy aged out and MAIN's did not.
// 0 = no limit.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VisionMaxAgeMs = 1000;
// Demand, recorded by the consumer, for the same reason the HUD records it: so that the producer
// can key on it without the two latching each other off. See the note in CyberpunkVR_GetHudTexture.
std::atomic<uint64_t> g_vision_consumed_tick{0};
// Demand-driven: the copy is ~35 MB a frame at 2560x2560x5mips, so it must not run when nothing
// consumes it. Measured with the headset off, it ran every single frame for nothing. The eye-1
// path stamps this each time it takes the texture; one bootstrap copy is allowed so the very
// first consumer has something to ask for, and after that the copy follows demand.
std::atomic<uint64_t> g_hud_consumed_tick{0};
// Which list has the HUD target bound, on this recording thread. Both are needed: the resource
// says WHAT to copy, the list says the pending bind still belongs to the list we are in, so a
// list abandoned mid-HUD can never make us barrier a resource that is no longer a render target.
thread_local ID3D12Resource* t_hud_rt_bound = nullptr;
thread_local ID3D12GraphicsCommandList* t_hud_rt_list = nullptr;

// Append, to the engine's OWN list, a copy of the finished HUD surface into a committed texture
// of ours. Recorded at the unbind, so in queue order it lands after the last HUD draw and before
// the mip chain -- and before any aliasing barrier can recycle the transient's heap memory, which
// is the whole reason this cannot be done later from the Present thread.
// enum HudSnapSlot now lives in Stereo/StereoInternal.hpp: the capture path files a bind
// under a slot, this file composites from them.

void hud_snapshot_copy(ID3D12GraphicsCommandList* list, ID3D12Resource* src,
                              int which, D3D12_RESOURCE_STATES rest) {
    if (!g_game_device || !list || !src) return;
    const CommandListVtableHook* e = command_list_hook_entry(list);
    D3D12_RESOURCE_DESC d{};
    if (!e || !e->barrier_call || !e->copyres || !mirror_get_resource_desc(src, &d)) {
        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugHudSnapSkips));
        return;
    }
    ID3D12Resource** slots[5] = { &g_hud_snap, &g_hud_blur_snap,
                                  &g_main_out_snap, &g_main_scene_snap, &g_vision_snap };
    D3D12_RESOURCE_DESC* descs[5] = { &g_hud_snap_desc, &g_hud_blur_desc,
                                      &g_main_out_desc, &g_main_scene_desc, &g_vision_desc };
    std::atomic<bool>* freshes[5] = { &g_hud_snap_fresh, &g_hud_blur_fresh,
                                      &g_main_out_fresh, &g_main_scene_fresh, &g_vision_fresh };
    static const wchar_t* names[5] = { L"CyberpunkVR_HudSnapshot", L"CyberpunkVR_HudBlurSnapshot",
                                       L"CyberpunkVR_MainOutSnapshot",
                                       L"CyberpunkVR_MainSceneSnapshot",
                                       L"CyberpunkVR_VisionSnapshot" };
    if (which < 0 || which > 4) return;
    ID3D12Resource*& slot = *slots[which];
    D3D12_RESOURCE_DESC& slotDesc = *descs[which];
    std::atomic<bool>& slotFresh = *freshes[which];
    ID3D12Resource* snap = nullptr;
    {
        std::lock_guard<std::mutex> lk(g_hud_snap_mtx);
        // Resolution or format change: intentionally leak the old snapshot rather than free a
        // texture a copy recorded this frame may still reference (rare, and it is 8 MB).
        //
        // "Rare" is load-bearing, and it stopped being true once a slot was fed by a detector
        // that matched two different sizes: the slot re-allocated on alternate calls and leaked
        // 1527 committed textures (~24 GB) in a single session. A caller that alternates is a
        // BUG in the caller, so cap the churn here and say so, rather than let it run away.
        if (slot && (slotDesc.Width != d.Width ||
                     slotDesc.Height != d.Height ||
                     slotDesc.Format != d.Format ||
                     slotDesc.MipLevels != d.MipLevels)) {
            static uint32_t s_realloc[5] = {0, 0, 0, 0, 0};
            if (++s_realloc[which] > 8) {
                if (s_realloc[which] == 9)
                    log("[hud] snapshot[%d] REFUSED: caller alternates between descs "
                        "(%llux%u fmt=%u vs %llux%u fmt=%u) -- ignoring further changes",
                        which, (unsigned long long)slotDesc.Width, slotDesc.Height,
                        (unsigned)slotDesc.Format, (unsigned long long)d.Width, d.Height,
                        (unsigned)d.Format);
                InterlockedIncrement64(
                    reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugHudSnapSkips));
                return;
            }
            slot = nullptr;
            slotFresh.store(false, std::memory_order_release);
        }
        if (!slot) {
            D3D12_HEAP_PROPERTIES hp{};
            hp.Type = D3D12_HEAP_TYPE_DEFAULT;
            hp.CreationNodeMask = hp.VisibleNodeMask = 1;
            D3D12_RESOURCE_DESC nd = d;
            nd.Flags = D3D12_RESOURCE_FLAG_NONE;      // plain copy target, sampled by us only
            // MAIN's finished frame holds sRGB-encoded bytes; typeless lets us decode on read.
            if (which == kSnapMainOut && nd.Format == DXGI_FORMAT_R8G8B8A8_UNORM)
                nd.Format = DXGI_FORMAT_R8G8B8A8_TYPELESS;
            ID3D12Resource* tex = nullptr;
            if (FAILED(g_game_device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &nd,
                    D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&tex))) || !tex) {
                InterlockedIncrement64(
                    reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugHudSnapSkips));
                return;
            }
            tex->SetName(names[which]);
            slot = tex;
            slotDesc = d;
            log("[hud] snapshot[%d]=%p %llux%u mips=%u fmt=%u", which, tex,
                (unsigned long long)d.Width, d.Height, (unsigned)d.MipLevels, (unsigned)d.Format);
        }
        snap = slot;
    }
    // The caller names the state the source is actually resting in; every snapshot here is taken
    // at a barrier where that is known exactly, so the transition below is never a guess.
    const D3D12_RESOURCE_STATES kHudRest = rest;
    D3D12_RESOURCE_BARRIER b[2]{};
    for (int i = 0; i < 2; ++i) {
        b[i].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b[i].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    }
    b[0].Transition.pResource = src;
    b[0].Transition.StateBefore = kHudRest;
    b[0].Transition.StateAfter  = D3D12_RESOURCE_STATE_COPY_SOURCE;
    b[1].Transition.pResource = snap;
    b[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
    b[1].Transition.StateAfter  = D3D12_RESOURCE_STATE_COPY_DEST;
    e->barrier_call(list, 2, b);
    e->copyres(list, snap, src);
    b[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    b[0].Transition.StateAfter  = kHudRest;
    b[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    b[1].Transition.StateAfter  = D3D12_RESOURCE_STATE_COMMON;
    e->barrier_call(list, 2, b);
    slotFresh.store(true, std::memory_order_release);
    if (which == kSnapHud) g_hud_snap_tick.store(GetTickCount64(), std::memory_order_release);
    InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugHudSnaps));
}

// ---- the engine's HUD composite constants ----------------------------------------------------
// Read out of the game's own b6 buffer at the composite dispatch (PipelineState_576), so these
// are the engine's live values, not a fit by eye. Exported so they can be tuned without a
// rebuild if a graphics setting turns out to move them.
extern "C" __declspec(dllexport) float CyberpunkVR_HudCurvatureX   = 0.009017f;
extern "C" __declspec(dllexport) float CyberpunkVR_HudCurvatureY   = 0.084242f;
extern "C" __declspec(dllexport) float CyberpunkVR_HudAberration   = 0.0001f;
extern "C" __declspec(dllexport) float CyberpunkVR_HudGain         = 2.0f;    // the engine's x2
extern "C" __declspec(dllexport) float CyberpunkVR_HudGlowGain     = 1.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_HudShadow       = 1.0f;
// The wide halo (map, weapon icons, tracked quest). Gain is the engine's _43_m0[5].x; the lod is
// its 1.8 rebased from the half-res pyramid it samples onto our full-res mip chain.
extern "C" __declspec(dllexport) float CyberpunkVR_HudBloomGain    = 0.65f;
extern "C" __declspec(dllexport) float CyberpunkVR_HudBloomLod     = 1.8f;
// Sharpness bisection: 0 = full composite, 1 = HUD term only, 2 = no curvature warp,
// 3 = no halo, 4 = no glow, 5 = no aberration. Live-switchable, no rebuild.
extern "C" __declspec(dllexport) float CyberpunkVR_HudDebugMode    = 0.0f;
// On, and on the engine's own frame time (bound as b1 from its own constant buffer). This is not
// cosmetic: the flicker term gates the glow, and its mean is about 0.66 -- forcing it to 1 made
// the halo half again too strong and visibly softened the text.
extern "C" __declspec(dllexport) float CyberpunkVR_HudFlicker      = 1.0f;

ColorBlit::HudParams hud_composite_params() {
    ColorBlit::HudParams p{};              // defaults are the captured values
    p.curvature[0]   = CyberpunkVR_HudCurvatureX;
    p.curvature[1]   = CyberpunkVR_HudCurvatureY;
    p.aberration     = CyberpunkVR_HudAberration;
    p.hudGain        = CyberpunkVR_HudGain;
    p.glowGain       = CyberpunkVR_HudGlowGain;
    p.shadowStrength = CyberpunkVR_HudShadow;
    p.bloomGain      = CyberpunkVR_HudBloomGain;
    p.bloomLod       = CyberpunkVR_HudBloomLod;
    p.flicker        = CyberpunkVR_HudFlicker;
    p.debugMode      = CyberpunkVR_HudDebugMode;
    (void)0;
    p.time           = (float)(GetTickCount64() % 100000ull) * 0.001f;
    return p;
}

// The finished HUD, premultiplied-alpha, in the game's output resolution. Null until the first
// frame that actually drew a HUD (loading screens, photo mode with HUD off, and so on).
// Taking it counts as demand -- see g_hud_consumed_tick.
// The second eye's own outline layer, premultiplied like the HUD surface. Null until a frame
// has actually drawn one (i.e. until something is being scanned).
extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetVisionTexture() {
    if (!CyberpunkVR_VisionSnap) return nullptr;
    // Shaped like CyberpunkVR_GetHudTexture below, line for line, because that path already solved
    // this: demand first and unconditionally, then freshness, then a liveness window that can be
    // widened or switched off. The differences from it were the bug, not a design.
    g_vision_consumed_tick.store(GetTickCount64(), std::memory_order_release);
    if (!g_vision_fresh.load(std::memory_order_acquire)) return nullptr;
    if (CyberpunkVR_VisionMaxAgeMs) {
        const uint64_t t = g_vision_tick.load(std::memory_order_acquire);
        if (!t || GetTickCount64() - t > CyberpunkVR_VisionMaxAgeMs) return nullptr;
    }
    std::lock_guard<std::mutex> lk(g_hud_snap_mtx);
    return g_vision_snap;
}

extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetHudTexture() {
    if (!CyberpunkVR_HudToSecondEye) return nullptr;
    // DEMAND IS RECORDED FIRST, AND UNCONDITIONALLY. The producer gate in hk_ResourceBarrier
    // keys on this tick -- it only takes a snapshot while someone is asking for one -- so it
    // must mean "the consumer asked", never "the consumer got something". Deriving it from the
    // return value instead makes the two latch each other off: one stale frame stops demand,
    // two seconds later production stops, and the snapshot can never become fresh again. That
    // is exactly what killed the HUD when the age check below was first added.
    g_hud_consumed_tick.store(GetTickCount64(), std::memory_order_release);
    if (!g_hud_snap_fresh.load(std::memory_order_acquire)) return nullptr;
    // Liveness, not existence. The fresh flag latches on the first copy and never clears, so when
    // the engine stops drawing a HUD -- a menu, a load -- the second eye would keep compositing
    // the last one it saw. 0 disables the limit.
    if (CyberpunkVR_HudMaxAgeMs) {
        const uint64_t t = g_hud_snap_tick.load(std::memory_order_acquire);
        if (!t || GetTickCount64() - t > CyberpunkVR_HudMaxAgeMs) return nullptr;
    }
    std::lock_guard<std::mutex> lk(g_hud_snap_mtx);
    return g_hud_snap;
}

// The engine's blurred-HUD pyramid, and the frame exposure the composite scales everything by
// (_1472). Both are the engine's own data, so the result is its arithmetic, not an imitation.
extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetHudBlurTexture() {
    if (!CyberpunkVR_HudToSecondEye) return nullptr;
    if (!g_hud_blur_fresh.load(std::memory_order_acquire)) return nullptr;
    std::lock_guard<std::mutex> lk(g_hud_snap_mtx);
    return g_hud_blur_snap;
}
extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetMainOutTexture() {
    if (!CyberpunkVR_HudToSecondEye) return nullptr;
    if (!g_main_out_fresh.load(std::memory_order_acquire)) return nullptr;
    std::lock_guard<std::mutex> lk(g_hud_snap_mtx);
    return g_main_out_snap;
}
extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetMainSceneTexture() {
    if (!CyberpunkVR_HudToSecondEye) return nullptr;
    if (!g_main_scene_fresh.load(std::memory_order_acquire)) return nullptr;
    std::lock_guard<std::mutex> lk(g_hud_snap_mtx);
    return g_main_scene_snap;
}
extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetHudExposureBuffer() {
    // MAIN's accumulator: the composite we are reproducing is MAIN's, so the HUD must be scaled
    // by the same exposure in both eyes or they would not match. Rests shader-readable.
    return g_expo_main.load(std::memory_order_acquire);
}
extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetFrameConstantBuffer() {
    return g_frame_cb.load(std::memory_order_acquire);
}
// Sweep the mapped upload rings for the composite's constants and take a private copy, so what
// gets bound is ours and cannot be recycled under us by the engine's ring allocator.
void hud_cb_rescan() {
    if (!g_game_device) return;
    const uint64_t now = GetTickCount64();
    if (g_hud_cb_scan_tick && now - g_hud_cb_scan_tick < 2000) return;
    g_hud_cb_scan_tick = now;

    float w = 0.0f, h = 0.0f;
    {
        std::lock_guard<std::mutex> lk(g_hud_snap_mtx);
        w = static_cast<float>(g_hud_snap_desc.Width);
        h = static_cast<float>(g_hud_snap_desc.Height);
    }
    if (!(w > 0.0f && h > 0.0f)) return;

    // Snapshot the map list and let the lock go: the sweep takes milliseconds and the render
    // thread registers uploads through the same lock.
    MappedUpload local[64]{};
    uint32_t localN = 0;
    {
        std::lock_guard<std::mutex> lk(g_upload_map_mtx);
        localN = g_upload_map_n;
        for (uint32_t i = 0; i < localN; ++i) local[i] = g_upload_maps[i];
    }
    // Stay on the block already in use for as long as it still qualifies. Searching afresh every
    // time is what made the look flip between two sets of values: the ring holds several blocks
    // that satisfy the fingerprint (previous frames' copies among them), and whichever came first
    // in the sweep won. Re-search only once the held block stops being valid.
    static const uint8_t* s_held = nullptr;
    const uint8_t* found = (s_held && hud_cb_block_plausible(s_held, w, h)) ? s_held : nullptr;
    for (uint32_t i = 0; i < localN && !found; ++i) {
        const MappedUpload& m = local[i];
        if (!m.ptr || m.size < 512) continue;
        for (uint64_t off = 0; off + 512 <= m.size; off += 256) {
            if (hud_cb_block_plausible(m.ptr + off, w, h)) { found = m.ptr + off; break; }
        }
    }
    // The ring genuinely moves the block every few frames, so re-finding it is normal and not
    // worth a log line. It was only ever a problem while the fingerprint could match a block of
    // zeros; with that rejected, every block it finds carries the same settings.
    s_held = found;
    if (!found) return;

    if (!g_hud_cb_copy) {
        D3D12_HEAP_PROPERTIES hp{};
        hp.Type = D3D12_HEAP_TYPE_UPLOAD;
        hp.CreationNodeMask = hp.VisibleNodeMask = 1;
        D3D12_RESOURCE_DESC bd{};
        bd.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
        bd.Width = 512;
        bd.Height = 1;
        bd.DepthOrArraySize = 1;
        bd.MipLevels = 1;
        bd.Format = DXGI_FORMAT_UNKNOWN;
        bd.SampleDesc.Count = 1;
        bd.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        ID3D12Resource* buf = nullptr;
        if (FAILED(g_game_device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &bd,
                D3D12_RESOURCE_STATE_GENERIC_READ, nullptr, IID_PPV_ARGS(&buf))) || !buf) {
            return;
        }
        void* mapped = nullptr;
        D3D12_RANGE none{0, 0};
        if (FAILED(buf->Map(0, &none, &mapped)) || !mapped) { buf->Release(); return; }
        buf->SetName(L"CyberpunkVR_HudConstants");
        g_hud_cb_copy = buf;
        g_hud_cb_copy_ptr = static_cast<uint8_t*>(mapped);
    }
    memcpy(g_hud_cb_copy_ptr, found, 512);
    // Take over even from an earlier capture: this buffer is ours, so it cannot be recycled by the
    // ring allocator under the shader, and the block was found by the full fingerprint. Only the
    // first hand-over logs; after that the copy above just refreshes the contents in place.
    if (!g_hud_cb_from_ring.load(std::memory_order_acquire)) {
        ID3D12Resource* prev = g_hud_cb.exchange(g_hud_cb_copy, std::memory_order_acq_rel);
        g_hud_cb_from_ring.store(true, std::memory_order_release);
        if (prev && prev != g_hud_cb_copy) prev->Release();
        CyberpunkVR_DebugHudCb = reinterpret_cast<uint64_t>(g_hud_cb_copy);
        const float* r = reinterpret_cast<const float*>(g_hud_cb_copy_ptr);
        log("[hud] composite constants found in the upload ring%s: target=%.0fx%.0f "
            "curvature=(%.6f, %.6f) glow=(%.3f, %.3f, %.3f) aberration=%.6f halo=(%.3f lod %.2f)",
            prev ? " (replacing the copy-path capture)" : "",
            r[16 * 4 + 2], r[16 * 4 + 3], r[3 * 4 + 0], r[3 * 4 + 1],
            r[8 * 4 + 2], r[8 * 4 + 3], r[9 * 4 + 0], r[6 * 4 + 3],
            r[5 * 4 + 0], r[5 * 4 + 1]);
    }
}

// WHICH INPUT IS THE HUD STILL WAITING FOR? The composite needs five, and when any is missing it
// falls back to a plain blit -- silently, which is why "the HUD takes a while to come up" was only
// ever a guess about which one was late. Logged once per changed combination, so a normal session
// prints two lines: what it waited for, and that it stopped waiting.
extern "C" __declspec(dllexport) void CyberpunkVR_NoteHudCompositeInputs(
        const void* hud, const void* blur, const void* expo,
        const void* frameCb, const void* hudCb) {
    const uint32_t mask = (hud ? 1u : 0u) | (blur ? 2u : 0u) | (expo ? 4u : 0u) |
                          (frameCb ? 8u : 0u) | (hudCb ? 16u : 0u);
    static std::atomic<uint32_t> s_last{0xFFFFFFFFu};
    if (s_last.exchange(mask, std::memory_order_acq_rel) == mask) return;
    if (mask == 31u) {
        log("[hud] composite inputs complete -- the second eye gets the HUD");
        return;
    }
    // WHEN THE SURFACE IS THE MISSING INPUT, SAY WHY IN THE SAME BREATH.
    //
    // "waiting on: surface" has appeared after every menu for weeks and never carried the one fact
    // that separates the possibilities. The HUD node keeps running -- its DrawHUD counter climbs --
    // so either it binds nothing shaped like the HUD, or it binds it and we cannot say what the
    // descriptor points at. The second is measurable and, going by the RTV map having silently
    // stopped accepting new descriptors at 2048, is the likely one: `bound` comes back null and
    // hud_adopt_by_node is simply never called.
    //
    // nodeBinds vs unresolved answers it outright, and the map fill says whether the map is why.
    if (!(mask & 1u)) {
        log("[hud] composite waiting on: surface%s%s%s%s | hud node binds=%llu of which"
            " unresolved=%llu | rtv map %u/%u%s",
            (mask & 2u) ? "" : " blur-pyramid", (mask & 4u) ? "" : " exposure",
            (mask & 8u) ? "" : " frame-constants", (mask & 16u) ? "" : " composite-constants",
            (unsigned long long)g_hud_node_binds.load(std::memory_order_relaxed),
            (unsigned long long)g_hud_node_unresolved.load(std::memory_order_relaxed),
            g_rtv_dim_count.load(std::memory_order_relaxed),
            (unsigned)g_rtv_dim_map.size(),
            g_rtv_dim_wrapped_logged ? " (wrapped)" : "");
        return;
    }
    log("[hud] composite waiting on:%s%s%s%s%s",
        (mask & 1u)  ? "" : " surface",
        (mask & 2u)  ? "" : " blur-pyramid",
        (mask & 4u)  ? "" : " exposure",
        (mask & 8u)  ? "" : " frame-constants",
        (mask & 16u) ? "" : " composite-constants");
}

extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetHudConstantBuffer() {
    hud_cb_rescan();
    return g_hud_cb.load(std::memory_order_acquire);
}

// ================================================================================================
// THE CAPABILITY GRANT, moved here from the monolith.
//
// A node runs for a view only if the view's capability mask permits it, and the HUD's node is one the
// second view is not granted by default. hud_grant_capability sets that bit; hud_dump_capability_mask
// prints what a work context actually asked for versus what it was given -- which is the only way to
// tell "the HUD node never ran" from "it ran and produced nothing".
//
// It belongs with the HUD rather than with the profiler it was sitting inside: nothing in it measures
// anything.
// ================================================================================================

extern "C" __declspec(dllexport) uint32_t CyberpunkVR_HudCapWord   = 11;      // qword index
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_HudCapBits   = 0x80;    // bit 7 (=711)
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudCapGrants = 0;

// Idempotent OR into the view's own capability mask. Deliberately NOT saved/restored: a
// capability that only exists while one node runs is exactly the half-measure that crashed.
void hud_grant_capability(uintptr_t ctx) {
    if (!CyberpunkVR_HudGrantCap || !ctx) return;
    const uint32_t w = CyberpunkVR_HudCapWord;
    const uint64_t bits = CyberpunkVR_HudCapBits;
    if (w >= 32 || !bits) return;
    __try {
        uint64_t* slot = reinterpret_cast<uint64_t*>(ctx + 6304) + w;
        if ((*slot & bits) != bits) {
            *slot |= bits;
            InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
                &CyberpunkVR_DebugHudCapGrants));
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}

void hud_dump_capability_mask(uint8_t* work_context, bool vrcam) {
    if (vrcam ? g_hud_mask_dumped_vrcam : g_hud_mask_dumped_main) return;
    if (!work_context || !g_exe_base) return;
    if (!g_hud_viewdata_get) {
        g_hud_viewdata_get = reinterpret_cast<HudViewDataFn>(g_exe_base + 0x1ED930);
    }
    __try {
        const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(work_context + 0x18);
        if (!ctx) return;
        const uint64_t* viewBits = reinterpret_cast<const uint64_t*>(ctx + 6304);
        const uint64_t* required =
            reinterpret_cast<const uint64_t*>(g_exe_base + HUD_REQUIRED_MASK_RVA + 8);
        if (vrcam) g_hud_mask_dumped_vrcam = true; else g_hud_mask_dumped_main = true;

        int missingWords = 0;
        for (int i = 0; i < 32; ++i) {
            const uint64_t req = required[i];
            const uint64_t have = viewBits[i];
            if (!req && !have) continue;
            const uint64_t missing = req & ~have;
            if (req || missing) {
                log("[hudmask] %s w%02d req=%016llX have=%016llX missing=%016llX",
                    vrcam ? "VRCAM" : "MAIN ", i,
                    (unsigned long long)req, (unsigned long long)have,
                    (unsigned long long)missing);
            }
            if (missing) ++missingWords;
        }
        log("[hudmask] %s summary: qwords with missing bits = %d (0 means this view would pass)",
            vrcam ? "VRCAM" : "MAIN ", missingWords);

        // Addresses for a hardware write-breakpoint on the block-list slot. Both views are built
        // by the same builder, so whatever fills viewData+0x168 for MAIN runs for VRCAM too and
        // bails on some condition -- and a write watchpoint on MAIN's slot names it in one shot,
        // which no amount of reading the graph will.
        void* vd = g_hud_viewdata_get ? g_hud_viewdata_get(work_context) : nullptr;
        void* blocks = nullptr;
        if (vd) blocks = *reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(vd) + 0x168);
        const uint32_t layerTag = *reinterpret_cast<uint32_t*>(work_context + 0x14);
        log("[hudmask] %s ctx=%p viewData=%p blocks(+0x168)=%p watch=%p layerTag=0x%02X",
            vrcam ? "VRCAM" : "MAIN ",
            reinterpret_cast<void*>(ctx), vd, blocks,
            vd ? reinterpret_cast<void*>(reinterpret_cast<uint8_t*>(vd) + 0x168) : nullptr,
            layerTag);
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}

}  // namespace detail
}  // namespace cvr
