// Capture -- finding the second view's finished image, and knowing what state it rests in.
//
// Everything downstream depends on this file being right: the mirror window, the OpenXR eye submit and
// the HUD composite all read what it identifies. And the failure mode is never an error -- it is a
// black frame, or the wrong texture, or a frame from two frames ago.
//
// THE IDENTIFICATION IS BY DESCRIPTOR AND BINDING, NEVER BY REMEMBERED POINTER. The engine reallocates
// its transients, so the same address names different surfaces across a graph rebuild. Hence the RTV
// maps here -- handle to resource, handle to dimensions, and a candidate ring; hence
// mirror_publish_output rejecting a candidate for four separate reasons and counting each; and hence
// the pinned and FOREIGN sets, which say "this resource is ours, never treat it as an engine target".
//
// WHY hk_ResourceBarrier IS IN THIS FILE. Copying from a resource in the wrong state gives BLACK, with
// no error from D3D12 and nothing in the log. The barrier hook tracks the state each resource actually
// rests in, so a copy can be issued from where the resource really is rather than where we assumed it
// would be. That is the same job as the identification above, one level down, which is why the two
// live together even though one is a vtable hook and the other is a set of maps.
//
// The vrcam DynamicTexture rests in PIXEL_SHADER_RESOURCE, not RENDER_TARGET. The note inside the
// moved block records both the value and the symptom of getting it wrong, because that fact was paid
// for rather than read.

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

// ===== VRCAM MIRROR OUTPUT ==================================================
// Standalone borderless window + own D3D12 swapchain that mirrors the vrcam RTT
// texture every frame (CopyResource -> our backbuffer -> Present). Capture in
// OBS via Window/Game Capture. Fully toggle-gated + self-disabling on any error
// so it can never take down the game. v1: quality/sync refined later.
// Default OFF: the mirror is a debug/streaming view, not part of the VR path, and it costs a
// second swapchain + a per-frame copy. Turned on from the overlay ("VRCAM Mirror"). Safe to
// flip at any time: the present thread is started lazily from the vrcam blit submit, so with
// this at 0 it simply never starts, and switching to 1 later starts it on the next vrcam frame.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_MirrorOutput      = 0;  // D3D11On12 mirror (OBS-style, non-blocking)
// The fixed resting-state GUESS that used to live here is DELETED. It was documented as a
// "fixed GUESS fallback only" and it is exactly the shape of lie the driver's state machine
// faults on -- see the note beside t_mirror_src_state_seen. There is a tracked state or
// there is no copy.
// The vrcam RenderFinal2D output's resting D3D12 state VARIES frame-to-frame. A fixed
// guess (CyberpunkVR_MirrorCopyState) makes the copy's transition barrier StateBefore
// wrong on mismatched frames -> hazard -> the copy reads stale/aliased heap memory
// (main's content, since vrcam's output shares the transient heap with main) -> the
// bright/dark alternation. hk_ResourceBarrier already tracks the resource's ACTUAL
// state (CyberpunkVR_DebugMirrorSrcState); use THAT for the copy barrier. 1=track (fix),
// 0=old fixed-guess behavior (for A/B).
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_MirrorTrackState = 1;
// Redirect the vrcam RenderFinal2D output RTV to our OWN committed target so the engine
// renders the final into a stable, never-aliased resource we control. View-aware (only
// inside the ctx-keyed vrcam RenderFinal2D node); no resolution heuristic -> VR-safe.
// Default 0 (enable live via x64dbg after deploy, so a bad build can't wedge startup).
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_VrcamOwnTarget    = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugOwnTargetSubs = 0;
// Diagnostic: when 1, the present thread clears the mirror window to RED and does
// NOT copy mtex. Red window => present/window pipeline works => black is the source
// copy. Black window => present/window itself is broken. Isolates the two halves.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_MirrorTestPattern = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMirrorState  = 0;  // 0 idle 1 finding 2 init 3 running 9 failed
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorRes    = 0;  // found vrcam ID3D12Resource
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMirrorW      = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMirrorH      = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorFrames = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMirrorLastHr = 0;

static std::atomic<bool> g_mirror_thread_started{false};




// THE __except HERE IS NOT A SAFETY NET, AND A CRASH DUMP PROVED IT.
//
//     EXCEPTION 0xC0000005  read at FFFFFFFFFFFFFFFF
//     RIP  CyberpunkVR_Stereo.dll +0x459AF     mov eax,[rcx+40h]     <- this line, rcx from +0x1E8
//     rcx  3E7DB6CC46CD6766
//
// The component pointer was stale -- a frame-graph rebuild had freed it -- so +0x1E8 read whatever
// the allocator left behind, the null test passed, and the dereference went to a NON-CANONICAL
// address. That is a #GP, not a page fault, which is why Windows reports the address as -1 and why
// no CR2 appears. The game's own vectored crash handler runs before any frame-based __except, so
// this handler never got the chance it was written for: the process was already writing a dump.
//
// Hence a plausibility test in front of every dereference. A live heap pointer here is canonical,
// above the first 64 KB, and 8-aligned; 3E7DB6CC46CD6766 fails the first of those outright. This is
// cheap, it runs before anything can fault, and it does not depend on being allowed to handle an
// exception that the host may claim first.
static inline bool mirror_ptr_plausible(uintptr_t p) {
    return p >= 0x10000u && p < 0x00007FFFFFFFFFFFull && (p & 7u) == 0;
}
static bool mirror_rd_dtex(uintptr_t comp, uintptr_t* dtex, UINT* w, UINT* h) {
    if (!mirror_ptr_plausible(comp)) return false;
    __try {
        uintptr_t d = *reinterpret_cast<uintptr_t*>(comp + 0x1E8);
        if (d && !mirror_ptr_plausible(d)) return false;   // freed component, garbage in the slot
        *dtex = d;
        if (d) { *w = *reinterpret_cast<UINT*>(d + 0x40); *h = *reinterpret_cast<UINT*>(d + 0x44); }
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

// BFS from the RTT dtex object: scan each object's first 0x400 bytes for a
// D3D12 TEXTURE2D whose dims match the dtex (vrcam output); enqueue heap ptrs.

// ---- Capture the persistent vrcam output -----------------------------------
// The cooked dtex is an R8G8B8A8_UNORM resource with an SRGB RTV. The old
// dummy-device copy hook rejected UNORM and is incompatible with SL's D3D12
// interposer. Hooks below are installed on the real game device instead.
std::atomic<ID3D12Resource*> g_captured_vrcam_res{nullptr};
static UINT g_target_w = 0, g_target_h = 0;

extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMirrorFmt = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorStage = 0;

static std::mutex g_mirror_resource_mtx;

// ---- D3D12 decoupled mirror (real second swapchain for VRCAM) --------------
// GAME thread appends ONE CopyResource(dtex -> g_d12_mtex) into the game's OWN
// blit command list (at the dtex's RENDER_TARGET->read barrier, so state is exact
// and there is no extra queue / allocator / cross-API sync). A dedicated thread
// owns its own command queue + D3D12 swapchain + window (message pump) and copies
// g_d12_mtex into its backbuffer and Presents -> a genuine capturable backbuffer,
// zero present cost on the game thread. (globals declared earlier.)
bool d12_mirror_ensure(const D3D12_RESOURCE_DESC& src) {
    if (g_d12_mtex) return true;
    std::lock_guard<std::mutex> lk(g_d12_mtx);
    if (g_d12_mtex) return true;
    if (!g_game_device) return false;
    D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_DEFAULT;
    hp.CreationNodeMask = 1; hp.VisibleNodeMask = 1;
    // ALLOW_RENDER_TARGET so the HUD debug overlay can be drawn straight onto the mirror image
    // (see hud_mirror_overlay). This is NOT the flag that broke 11on12 sharing before -- that was
    // ALLOW_SIMULTANEOUS_ACCESS; a wrapped resource being a render target is ordinary.
    D3D12_RESOURCE_DESC d = src;
    d.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    ID3D12Resource* tex = nullptr;
    if (FAILED(g_game_device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d,
            D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&tex))) || !tex) {
        // Fall back to the historical plain target: the mirror is worth more than the overlay.
        d.Flags = D3D12_RESOURCE_FLAG_NONE;
        if (FAILED(g_game_device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d,
                D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&tex))) || !tex) return false;
        log("[mirror] mirror-tex has no RENDER_TARGET flag -- HUD debug overlay disabled");
    } else {
        g_d12_mtex_is_rt = true;
    }
    if (!g_d12_fence && FAILED(g_game_device->CreateFence(0, D3D12_FENCE_FLAG_NONE,
            IID_PPV_ARGS(&g_d12_fence)))) { tex->Release(); return false; }
    tex->SetName(L"CyberpunkVR_MirrorTex");
    g_d12_w = (UINT)d.Width; g_d12_h = d.Height; g_d12_fmt = d.Format;
    g_d12_mtex = tex;
    CyberpunkVR_DebugMirrorStage = reinterpret_cast<uint64_t>(tex);
    log("[mirror] d12 mirror-tex=%p %ux%u fmt=%u", tex, g_d12_w, g_d12_h, (unsigned)g_d12_fmt);
    return true;
}

static bool mirror_rgba8_fmt(DXGI_FORMAT f) {
    return f == DXGI_FORMAT_R8G8B8A8_UNORM
        || f == DXGI_FORMAT_B8G8R8A8_UNORM
        || f == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
        || f == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
        || f == DXGI_FORMAT_R8G8B8A8_TYPELESS
        || f == DXGI_FORMAT_B8G8R8A8_TYPELESS
        // vrcam final RenderFinal2D output is an HDR packed float target:
        || f == DXGI_FORMAT_R11G11B10_FLOAT
        || f == DXGI_FORMAT_R16G16B16A16_FLOAT
        || f == DXGI_FORMAT_R10G10B10A2_UNORM;
}

bool mirror_get_resource_desc(ID3D12Resource* resource,
        D3D12_RESOURCE_DESC* desc) {
    if (!resource || !desc) return false;
    __try {
        *desc = resource->GetDesc();
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

static bool mirror_target_dimensions(const D3D12_RESOURCE_DESC& desc) {
    if (desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D) return false;
    // PRIMARY source: the resolution encoded in the SELECTED component's name. This used to
    // fall back to a literal 2444x2444, which silently killed the mirror at every other
    // resolution: no render target ever matched, so no RTV was ever registered as a candidate,
    // so the vrcam target was never captured and the present thread waited forever for a
    // texture that was never created. Nothing downstream logged anything, because nothing
    // downstream ever ran.
    UINT tw = g_vrcam_sel_w.load(std::memory_order_relaxed);
    UINT th = g_vrcam_sel_h.load(std::memory_order_relaxed);
    if (!tw || !th) {
        // Fallback: read the dims off the live RTT component, when we have its address.
        if (!g_target_w) {
            uintptr_t comp = static_cast<uintptr_t>(CyberpunkVR_DebugRttComp);
            uintptr_t dtex = 0; UINT w = 0, h = 0;
            if (comp && mirror_rd_dtex(comp, &dtex, &w, &h) && w && h) {
                g_target_w = w;
                g_target_h = h;
            }
        }
        tw = g_target_w; th = g_target_h;
    }
    if (!tw || !th) {
        static bool warned = false;          // loud once, instead of matching a wrong size
        if (!warned) {
            warned = true;
            log("[mirror] no VRCAM resolution known (component=%s) -> RTV capture disabled",
                g_vrcam_component);
        }
        return false;
    }
    // ONE ACCEPTED SIZE, THE ONE FROM THE COMPONENT NAME. Nothing else.
    //
    // 0.2.2 added a second answer here -- the dtex dims read live off the RTT component -- to fix
    // the eye going mono after the inventory. It fixed nothing and broke two things, and all three
    // facts are measured rather than argued:
    //
    //   * It was never needed. The RTT stayed 3072x3072 for entire sessions, exactly what the name
    //     vrcam_3072x3072 parses to, so this predicate had not rejected a single target.
    //   * "Purely additive" was true of the RETURN VALUE and false of everything downstream. This
    //     predicate also gates REGISTRATION, so when the live read came back 1x5 every 1x5 RGBA8
    //     target in the engine became a vrcam-output candidate. Forty a second poured into the
    //     512-slot table, the cursor wrapped, the one entry that mattered was evicted, and the
    //     second eye went mono -- the symptom it was written to cure.
    //   * The read itself killed the process. A frame-graph rebuild frees the component, so
    //     CyberpunkVR_DebugRttComp dangles; +0x1E8 then holds allocator debris, and following it
    //     landed on a non-canonical address. That is the 0xC0000005-at-FFFFFFFFFFFFFFFF dump, and
    //     the 1x5 was the same debris on a luckier frame.
    //
    // So it is gone rather than patched. A guess that has to be sanity-checked against garbage it
    // should not be reading is not a fallback. If a rebuild ever does move the RTT for real, the
    // fix belongs where the size is CHOSEN -- re-derived from the component the way the name is
    // parsed -- not in a per-bind predicate following a pointer nobody owns.
    return (UINT)desc.Width == tw && desc.Height == th;
}

// The vrcam outputs that have actually been published, readable without taking a lock so the
// eviction picker below can consult it on the descriptor-creation path. Append-only and the
// entries are AddRef'd for the session, so a relaxed scan is safe: a pointer read here is either
// null or a resource that outlives the read.
static std::array<std::atomic<ID3D12Resource*>, 64> g_mirror_pinned{};
static std::atomic<uint32_t> g_mirror_pinned_n{0};
static bool mirror_is_pinned_output(ID3D12Resource* r) {
    if (!r) return false;
    uint32_t n = g_mirror_pinned_n.load(std::memory_order_acquire);
    if (n > g_mirror_pinned.size()) n = (uint32_t)g_mirror_pinned.size();
    for (uint32_t i = 0; i < n; ++i)
        if (g_mirror_pinned[i].load(std::memory_order_relaxed) == r) return true;
    return false;
}

struct MirrorRtvCandidate {
    SIZE_T handle = 0;
    ID3D12Resource* resource = nullptr;
    DXGI_FORMAT resource_format = DXGI_FORMAT_UNKNOWN;
    DXGI_FORMAT view_format = DXGI_FORMAT_UNKNOWN;
};
// Sized for BOTH views: in VR, MAIN renders at the same resolution as VRCAM, so its render
// targets pass the same coarse dimension filter and land here too. That is harmless for
// correctness -- the actual VRCAM discrimination is the node gate in hk_OMSetRenderTargets,
// which only captures while the vrcam RenderFinal2D node is on the stack -- but with a small
// table the vrcam entry could be refused once MAIN had filled it. Hence the ring below:
// running out of slots must never be able to drop the one target we need.
static std::array<MirrorRtvCandidate, 512> g_mirror_rtv_candidates{};
static std::atomic<uint32_t> g_mirror_rtv_candidate_count{0};
static std::atomic<uint32_t> g_mirror_rtv_next{0};      // ring cursor once full
static std::mutex g_mirror_rtv_candidate_mtx;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorRtvRegs = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorRtvEvicts = 0;
// Countdown of RTV binds inside the vrcam node still to be logged (see the [rtvpick]
// diagnostic in hk_OMSetRenderTargets). Burns down to 0 so it costs nothing after the
// first frames; raise it live from the debugger to sample again.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DebugRtvPickLog = 48;

// ---- resources owned by US, not by the engine ---------------------------------------
// The mirror recognises the VRCAM render target by shape alone: an RGBA8-family (or
// packed-HDR) 2D texture whose size equals the selected VRCAM resolution. That was
// unambiguous while the engine was the only thing creating such textures.
//
// It stopped being unambiguous once the OpenXR submit path came in: its per-eye capture
// textures are deliberately the SAME size as VRCAM, in an RGBA8 family, and eye 1 gets a
// render-target view (the sRGB encode blit). So they matched the heuristic, got
// registered as VRCAM candidates, and the mirror started sampling OUR half-written
// capture instead of the engine's output -- which is exactly the "left side fine, right
// side a different pass, bottom-right black" corruption, visible in the desktop mirror
// because the fault is upstream of any submit.
//
// Anything we create ourselves registers here and is skipped by the heuristics.
static std::array<std::atomic<ID3D12Resource*>, 32> g_foreign_res{};

extern "C" __declspec(dllexport) void CyberpunkVR_RegisterForeignResource(ID3D12Resource* r) {
    if (!r) return;
    for (auto& slot : g_foreign_res) {
        ID3D12Resource* cur = slot.load(std::memory_order_acquire);
        if (cur == r) return;                       // already known
        if (cur) continue;
        ID3D12Resource* expected = nullptr;
        if (slot.compare_exchange_strong(expected, r, std::memory_order_acq_rel)) {
            log("[mirror] foreign resource registered %p (excluded from VRCAM detection)", r);
            return;
        }
    }
    // Full table: log rather than silently letting the next one through, because a miss
    // here shows up as mirror/eye corruption and would be maddening to trace.
    log("[mirror] WARNING foreign-resource table full, %p NOT excluded", r);
}

static bool mirror_is_foreign(ID3D12Resource* r) {
    if (!r) return false;
    for (const auto& slot : g_foreign_res) {
        ID3D12Resource* cur = slot.load(std::memory_order_acquire);
        if (!cur) break;                            // filled in order; first null ends it
        if (cur == r) return true;
    }
    return false;
}

static void mirror_register_rtv(ID3D12Resource* resource, DXGI_FORMAT view_format,
        D3D12_CPU_DESCRIPTOR_HANDLE handle) {
    D3D12_RESOURCE_DESC desc{};
    if (!handle.ptr || mirror_is_foreign(resource) || !mirror_rgba8_fmt(view_format) ||
        !mirror_get_resource_desc(resource, &desc) ||
        !mirror_rgba8_fmt(desc.Format) || !mirror_target_dimensions(desc)) {
        return;
    }
    std::lock_guard<std::mutex> lock(g_mirror_rtv_candidate_mtx);
    uint32_t count = g_mirror_rtv_candidate_count.load(std::memory_order_relaxed);
    for (uint32_t i = 0; i < count; ++i) {
        if (g_mirror_rtv_candidates[i].handle == handle.ptr &&
            g_mirror_rtv_candidates[i].resource == resource) {
            return;
        }
        // Same descriptor slot, different resource: the engine recycled the descriptor, so the
        // old entry is stale and must go rather than shadow the new one on lookup.
        if (g_mirror_rtv_candidates[i].handle == handle.ptr) {
            if (g_mirror_rtv_candidates[i].resource) g_mirror_rtv_candidates[i].resource->Release();
            resource->AddRef();
            g_mirror_rtv_candidates[i] = { handle.ptr, resource, desc.Format, view_format };
            ++CyberpunkVR_DebugMirrorRtvRegs;
            return;
        }
    }
    uint32_t slot;
    if (count < g_mirror_rtv_candidates.size()) {
        slot = count;
        g_mirror_rtv_candidate_count.store(count + 1, std::memory_order_release);
    } else {
        // THE RING MUST NOT BE ABLE TO DROP THE ONE TARGET WE NEED, which is what the comment on
        // the table above has always claimed and what the plain wrapping cursor did not deliver.
        // A wrap threw out the vrcam output's entry, its handle stopped resolving, and the second
        // eye went mono with the node still binding it every frame -- measured, evicts climbing
        // 200 per five seconds against a 512-slot table.
        //
        // So the cursor walks past any slot holding a resource we have published as a vrcam
        // output. At most 64 of those against 512 slots, so a victim is always found; the loop is
        // bounded anyway and falls back to evicting the cursor's own slot rather than spinning.
        const uint32_t n = (uint32_t)g_mirror_rtv_candidates.size();
        slot = g_mirror_rtv_next.fetch_add(1, std::memory_order_relaxed) % n;
        for (uint32_t tries = 0; tries < n; ++tries) {
            if (!mirror_is_pinned_output(g_mirror_rtv_candidates[slot].resource)) break;
            slot = g_mirror_rtv_next.fetch_add(1, std::memory_order_relaxed) % n;
        }
        static bool s_wrapLogged = false;
        if (!s_wrapLogged) {
            s_wrapLogged = true;
            log("[mirror] the RTV candidate table (%u) has wrapped -- evicting from now on, "
                "published vrcam outputs excepted", n);
        }
        if (g_mirror_rtv_candidates[slot].resource) {
            g_mirror_rtv_candidates[slot].resource->Release();
            ++CyberpunkVR_DebugMirrorRtvEvicts;
        }
    }
    resource->AddRef();
    g_mirror_rtv_candidates[slot] = { handle.ptr, resource, desc.Format, view_format };
    ++CyberpunkVR_DebugMirrorRtvRegs;
}

static ID3D12Resource* mirror_find_bound_rtv(SIZE_T handle,
        DXGI_FORMAT* view_format) {
    const uint32_t count =
        g_mirror_rtv_candidate_count.load(std::memory_order_acquire);
    for (uint32_t i = count; i > 0; --i) {
        const MirrorRtvCandidate& candidate = g_mirror_rtv_candidates[i - 1];
        if (candidate.handle == handle) {
            if (view_format) *view_format = candidate.view_format;
            return candidate.resource;
        }
    }
    return nullptr;
}

// Broad RTV->dims (any format/size). Populated from hk_CreateRTV; read by hk_OMSetRenderTargets.
static void rtv_dim_register(D3D12_CPU_DESCRIPTOR_HANDLE handle, ID3D12Resource* res) {
    if (!handle.ptr || !res) return;
    D3D12_RESOURCE_DESC d{};
    if (!mirror_get_resource_desc(res, &d) ||
        d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D) return;
    const uint32_t cap = (uint32_t)g_rtv_dim_map.size();
    std::lock_guard<std::mutex> lk(g_rtv_dim_mtx);
    uint32_t n = g_rtv_dim_count.load(std::memory_order_relaxed);
    for (uint32_t i = 0; i < n; ++i) {
        if (g_rtv_dim_map[i].handle.load(std::memory_order_relaxed) == handle.ptr) {
            // Same descriptor, re-created. Take it out of service while the fields change, so a
            // concurrent lookup cannot pair this handle with the previous resource.
            g_rtv_dim_map[i].handle.store(0, std::memory_order_release);
            g_rtv_dim_map[i].w = (uint32_t)d.Width;
            g_rtv_dim_map[i].h = d.Height;
            g_rtv_dim_map[i].res = res;
            g_rtv_dim_map[i].handle.store(handle.ptr, std::memory_order_release);
            return;
        }
    }
    uint32_t slot;
    if (n < cap) {
        slot = n;
    } else {
        slot = g_rtv_dim_next.fetch_add(1, std::memory_order_relaxed) % cap;
        if (!g_rtv_dim_wrapped_logged) {
            g_rtv_dim_wrapped_logged = true;
            log("[mirror] the RTV descriptor map (%u) has wrapped -- the oldest descriptors stop "
                "resolving from here. HUD identification and vrcam capture both read it.", cap);
        }
    }
    g_rtv_dim_map[slot].handle.store(0, std::memory_order_release);
    g_rtv_dim_map[slot].w = (uint32_t)d.Width;
    g_rtv_dim_map[slot].h = d.Height;
    g_rtv_dim_map[slot].res = res;
    g_rtv_dim_map[slot].handle.store(handle.ptr, std::memory_order_release);
    if (n < cap) g_rtv_dim_count.store(n + 1, std::memory_order_release);
}
static ID3D12Resource* rtv_resource_lookup(SIZE_T handle) {
    const uint32_t n = g_rtv_dim_count.load(std::memory_order_acquire);
    for (uint32_t i = n; i > 0; --i)
        if (g_rtv_dim_map[i - 1].handle.load(std::memory_order_acquire) == handle)
            return g_rtv_dim_map[i - 1].res;
    return nullptr;
}

static bool rtv_dim_lookup(SIZE_T handle, uint32_t* w, uint32_t* h) {
    const uint32_t n = g_rtv_dim_count.load(std::memory_order_acquire);
    for (uint32_t i = n; i > 0; --i) {
        if (g_rtv_dim_map[i - 1].handle.load(std::memory_order_acquire) == handle) {
            *w = g_rtv_dim_map[i - 1].w; *h = g_rtv_dim_map[i - 1].h;
            return true;
        }
    }
    return false;
}

// 64, NOT 8, AND IT SAYS SO WHEN IT FILLS.
//
// This is the AddRef list for the vrcam output targets. Eight was sized for the ring the engine
// rotates in a steady graph -- and a session log reached ring[7] with two more arriving back to
// back at the end, so it was full. Past that the resource is still tracked as the current output
// but never retained, which is a raw pointer into something the engine is free to destroy.
//
// The graph rebuilds are what overruns it: every map or inventory open builds a new graph with new
// dtex targets, so the set grows with the number of menus opened rather than with the ring size.
// Exactly the shape of the upload-map bug fixed earlier -- eight slots, silently full, everything
// after it dropped on the floor with no line in the log to say so. Same fix, and the same
// saturation notice, because the silence was the expensive part.
static ID3D12Resource* g_mirror_seen[64] = {};
static int g_mirror_seen_n = 0;
static bool g_mirror_seen_full_logged = false;
// Update the current vrcam output EVERY frame: the engine writes the vrcam final
// into a small ring of persistent committed dtex targets and rotates them, so
// capturing once => stale/black on the other frames. AddRef each unique resource
// once (they live for the whole session) so the copy path can safely use it.
// WHY THE OUTPUT WENT QUIET, ANSWERED BY THE LOG INSTEAD OF BY GUESSWORK.
//
// The second view keeps rendering after the inventory closes -- measured: view-create still firing
// at hits=4201, the vrcam DrawHUD counter still climbing -- while the headset falls to mono. So
// what stops is the CAPTURE of its output, here, and every way out of this function is silent.
// Three guesses have already been spent on which one it is. These counters and the watchdog below
// cost nothing and end the argument: whichever number is climbing when the eye goes stale names
// the branch.
static uint64_t g_pub_rej_foreign = 0, g_pub_rej_desc = 0, g_pub_rej_dims = 0, g_pub_same = 0;
static uint64_t g_pub_ok = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPubOk = 0;

 void mirror_publish_output(ID3D12Resource* resource,
        DXGI_FORMAT view_format) {
    // Watchdog: the output has not been refreshed for a second while binds keep arriving. That is
    // the state the headset shows as mono, so say it once with the tally that explains it.
    {
        static uint64_t s_lastOkMs = 0, s_lastWarnMs = 0;
        const uint64_t now = GetTickCount64();
        if (!s_lastOkMs) s_lastOkMs = now;
        if (g_pub_ok != CyberpunkVR_DebugPubOk) { CyberpunkVR_DebugPubOk = g_pub_ok; s_lastOkMs = now; }
        if (now - s_lastOkMs > 1000 && now - s_lastWarnMs > 5000) {
            s_lastWarnMs = now;
            log("[mirror] vrcam output has not been accepted for %llu ms -- the second eye is "
                "going stale. rejected: foreign=%llu desc=%llu dims=%llu, unchanged=%llu, "
                "accepted=%llu",
                (unsigned long long)(now - s_lastOkMs),
                (unsigned long long)g_pub_rej_foreign, (unsigned long long)g_pub_rej_desc,
                (unsigned long long)g_pub_rej_dims, (unsigned long long)g_pub_same,
                (unsigned long long)g_pub_ok);
        }
    }
    if (!resource || mirror_is_foreign(resource)) { ++g_pub_rej_foreign; return; }
    D3D12_RESOURCE_DESC desc{};
    if (!mirror_get_resource_desc(resource, &desc)) { ++g_pub_rej_desc; return; }
    if (!mirror_target_dimensions(desc)) { ++g_pub_rej_dims; return; }
    ++g_pub_ok;
    // Same resource as last time is the NORMAL case on a still frame, not a failure -- counted
    // separately so it cannot be mistaken for one in the tally above.
    if (g_captured_vrcam_res.load(std::memory_order_acquire) == resource) { ++g_pub_same; return; }
    {
        std::lock_guard<std::mutex> lk(g_mirror_resource_mtx);
        bool seen = false;
        for (int i = 0; i < g_mirror_seen_n; ++i)
            if (g_mirror_seen[i] == resource) { seen = true; break; }
        if (!seen && g_mirror_seen_n < static_cast<int>(std::size(g_mirror_seen))) {
            resource->AddRef();
            g_mirror_seen[g_mirror_seen_n++] = resource;
            // Pin it against RTV-table eviction. The list above exists to hold a reference; this
            // publishes the same set lock-free so mirror_register_rtv can refuse to throw the
            // entry away when the table wraps -- the failure that took the second eye.
            const uint32_t pin = g_mirror_pinned_n.load(std::memory_order_relaxed);
            if (pin < g_mirror_pinned.size()) {
                g_mirror_pinned[pin].store(resource, std::memory_order_relaxed);
                g_mirror_pinned_n.store(pin + 1, std::memory_order_release);
            }
            log("[mirror] vrcam output ring[%d]=%p %llux%u fmt=%u rtv=%u",
                g_mirror_seen_n - 1, resource, (unsigned long long)desc.Width,
                desc.Height, (unsigned)desc.Format, (unsigned)view_format);
        } else if (!seen && !g_mirror_seen_full_logged) {
            g_mirror_seen_full_logged = true;
            log("[mirror] vrcam output list FULL at %d -- further targets are used without a "
                "reference. If the second eye starts dropping to mono, this is the first place "
                "to look.", g_mirror_seen_n);
        }
    }
    g_captured_vrcam_res.store(resource, std::memory_order_release);
    CyberpunkVR_DebugMirrorFmt = (uint32_t)desc.Format;
    CyberpunkVR_DebugMirrorRes = reinterpret_cast<uint64_t>(resource);
}

// ---- THE HUD IS ITS OWN TEXTURE, AND IT CAN SIMPLY BE COPIED ---------------------------------
//
// Settled from two independent Nsight captures rather than from theory. The HUD is NOT drawn into
// the scene colour; the composition group renders it into a dedicated full-resolution surface:
//
//     DiscardResource(hudTex, NumSubresources = 5)
//     ClearRenderTargetView(mip0, {0,0,0,0})        <- cleared TRANSPARENT
//     OMSetRenderTargets(1, mip0); RSSetViewports(output res)
//     ... ~29 ink quad draws (stride-24 verts, blend ONE / INV_SRC_ALPHA) ...
//     OMSetRenderTargets(0, nullptr)                <- the snapshot below goes exactly here
//     ... mips 1..4 generated for the HUD glow ...
//
// and RenderFinal2D then composites that surface over the scene in a single fullscreen draw.
//
// Those draws are recorded into the "PostFX" command list, OUTSIDE the DrawHUD node's dynamic
// extent -- which is why the earlier node-scoped OMSetRenderTargets probe saw nothing and I
// concluded, wrongly, that no separable HUD surface existed. It does.
//
// Identification is exact, not heuristic: across all 23209 resources in the capture, exactly ONE
// is an RGBA8 render target with MipLevels != 1. That is the whole signature.
//
// Blend ONE / INV_SRC_ALPHA means the surface holds PREMULTIPLIED alpha, so the second eye wants
//     out.rgb = hud.rgb + eye.rgb * (1 - hud.a)
// which is what ColorBlit::RecordOverlay does.
//
// Nothing here touches engine state: the copy is a barrier pair around a CopyResource appended to
// the engine's own list, the same shape as mirror_stable_inline_copy, which has been stable.
extern "C" __declspec(dllexport) int      CyberpunkVR_HudToSecondEye   = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudSnaps    = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudSnapSkips = 0;

// The HUD subsystem moved to src/Stereo/Hud.cpp.

// The per-view constant-buffer probes moved to src/Stereo/ViewConstants.cpp.

 void STDMETHODCALLTYPE hk_CreateCBV(ID3D12Device* self,
        const D3D12_CONSTANT_BUFFER_VIEW_DESC* desc, D3D12_CPU_DESCRIPTOR_HANDLE dst) {
    g_orig_CreateCBV(self, desc, dst);
    // ---- the camera constant buffer, per view -------------------------------------------------
    // The one number the whole sight question now turns on: where each view's camera actually is,
    // read on the GPU side at the same instant as the weapon's world position.
    //
    // Why here: b1 is bound straight out of the upload ring, so no CopyBufferRegion ever carries
    // it -- the same reason the HUD composite's constants had to be found by scanning. But
    // CreateConstantBufferView hands us its GPU address, and the ring is already mapped.
    //
    // Identified by CONTENT, not by size: rows 40..42 are an orthonormal basis, row 37.w is
    // exactly 1.0, and row 36 equals row 37. Nothing else in the engine's constant traffic looks
    // like that, and it is true at any resolution.
    // No counter on the unfiltered path: this site fires ~32 million times a session, and an
    // atomic on every one of them is real frame time spent on nothing.
    if (CyberpunkVR_CamCbProbe && desc) {
        if (desc->SizeInBytes >= 768) {
            g_cc_big.fetch_add(1, std::memory_order_relaxed);
            // Two routes to the bytes: the ring if it is mapped, else the record of what was
            // copied into it. Neither is guaranteed for a buffer bound straight out of the ring,
            // which is exactly why the counters above exist.
            const uint8_t* cp = upload_cpu_for_va(desc->BufferLocation, 848);
            if (!cp) cp = filled_cpu_for_va(desc->BufferLocation, 848);
            if (cp) camcb_note(cp, t_vrcam_node_active);
            else camcb_stages();
        }
    }
    // Grading-LUT capture first: it wants every CBV made inside GenerateTonemappingLUT, of any
    // size, not just the 256-byte view blocks the older probe filtered for.
    if (CyberpunkVR_GradeCbProbe && t_grade_cb_view >= 0 && desc &&
        desc->SizeInBytes && desc->SizeInBytes <= GRADE_CB_MAX &&
        t_grade_cb_idx < GRADE_CB_SLOTS) {
        const uint8_t* gp = upload_cpu_for_va(desc->BufferLocation, desc->SizeInBytes);
        if (gp) {
            uint8_t tmp[GRADE_CB_MAX];
            if (cloud_cb_raw_copy(tmp, gp, desc->SizeInBytes)) {
                const int v = t_grade_cb_view;
                const uint32_t k = t_grade_cb_idx;
                std::lock_guard<std::mutex> lk(g_gcb_mtx);
                memcpy(g_gcb[v][k], tmp, desc->SizeInBytes);
                g_gcb_len[v][k] = desc->SizeInBytes;
            }
            ++t_grade_cb_idx;
        }
    }
    if (!CyberpunkVR_CbvProbe || !desc || desc->SizeInBytes != 256 || !g_exe_base) return;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    const uintptr_t work = t_current_node_work;
    if (work <= base) return;
    const uint8_t* p = upload_cpu_for_va(desc->BufferLocation, 256);
    if (!p) return;
    float xy[2]; uint32_t cnt = 0;
    if (!cbv_read_head(p, xy, &cnt)) return;     // not a view-constant block
    const uint32_t rva = static_cast<uint32_t>(work - base);
    cbv_probe_note(rva, cnt, t_vrcam_node_active);
    if (rva == CyberpunkVR_CbvDumpNode) cbv_dump_note(p, t_vrcam_node_active);
}

 void STDMETHODCALLTYPE hk_CreateRTV(ID3D12Device* self, ID3D12Resource* res,
        const D3D12_RENDER_TARGET_VIEW_DESC* desc, D3D12_CPU_DESCRIPTOR_HANDLE dst) {
    g_orig_CreateRTV(self, res, desc, dst);
    rtv_dim_register(dst, res);   // broad map (any format) for the crop-blit RT-size probe
    hud_register_rtv(res, desc, dst);
    if (desc) {
        mirror_register_rtv(res, desc->Format, dst);
    } else if (res) {
        D3D12_RESOURCE_DESC resource_desc{};
        if (mirror_get_resource_desc(res, &resource_desc))
            mirror_register_rtv(res, resource_desc.Format, dst);
    }
}

// Lazily create a committed texture matching `src`'s desc (in RENDER_TARGET) + an RTV in
// our own heap, so the engine can render the vrcam final directly into it. One target
// (single vrcam view for now). Returns the RTV handle, {0} on failure.
static D3D12_CPU_DESCRIPTOR_HANDLE mirror_ensure_own_target_rtv(
        ID3D12Resource* src, DXGI_FORMAT view_format) {
    D3D12_CPU_DESCRIPTOR_HANDLE none{0};
    if (!g_game_device || !src) return none;
    std::lock_guard<std::mutex> lk(g_own_target_mtx);
    if (g_own_target) return g_own_rtv;
    D3D12_RESOURCE_DESC d{};
    if (!mirror_get_resource_desc(src, &d)) return none;
    D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_DEFAULT;
    hp.CreationNodeMask = 1; hp.VisibleNodeMask = 1;
    d.Flags |= D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    ID3D12Resource* tex = nullptr;
    if (FAILED(g_game_device->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &d,
            D3D12_RESOURCE_STATE_RENDER_TARGET, nullptr, IID_PPV_ARGS(&tex))) || !tex)
        return none;
    D3D12_DESCRIPTOR_HEAP_DESC hd{}; hd.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    hd.NumDescriptors = 1;
    ID3D12DescriptorHeap* heap = nullptr;
    if (FAILED(g_game_device->CreateDescriptorHeap(&hd, IID_PPV_ARGS(&heap))) || !heap) {
        tex->Release(); return none;
    }
    D3D12_RENDER_TARGET_VIEW_DESC rd2{};
    rd2.Format = (view_format != DXGI_FORMAT_UNKNOWN) ? view_format : d.Format;
    rd2.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2D;
    D3D12_CPU_DESCRIPTOR_HANDLE h = heap->GetCPUDescriptorHandleForHeapStart();
    g_game_device->CreateRenderTargetView(tex, &rd2, h);
    tex->SetName(L"CyberpunkVR_VrcamOwnTarget");
    g_own_target = tex; g_own_rtv_heap = heap; g_own_rtv = h;
    log("[owntgt] committed vrcam target=%p %llux%u fmt=%u", tex,
        (unsigned long long)d.Width, d.Height, (unsigned)rd2.Format);
    return g_own_rtv;
}

// The vision / outline layer moved to src/Stereo/VisionLayer.cpp.

// ---- IS THE SUN-CASCADE ATLAS ONE TEXTURE FOR BOTH EYES? -----------------------------------------
//
// Asked because two attempts to make one eye reuse the other's cascades both failed, in opposite
// directions, and because the answer decides whether "hand the shadows from MAIN to the second view" is a
// fix or a contradiction:
//
//   * one texture  -> both eyes already rasterise into it, from a cascade record the in-frame probe shows
//                     identical, so they already sample the SAME shadow map. There is nothing to hand over
//                     and the map cannot be the source of the eye difference at all.
//   * two textures -> reuse would mean something, and the two maps' CONTENT becomes worth suspecting even
//                     though their inputs are measured identical.
//
// Everything said about this so far has been inference from the port's own comments ("reference the existing
// cascade atlas") and from how the failures looked. This measures it: the cascade pass binds a depth target,
// so the DSV descriptor it binds is recorded per view and per cascade index.
//
// MEASURED: identical, both cascades, both views -- casc0 M=197E7A25A60 V=197E7A25A60, casc1 ...A68 for
// both. (One earlier report showed casc1 differing; that was a transition, MAIN's handle coming from another
// part of the heap, and by the settled report both agreed.)
//
// AND THE CLAIM I DREW FROM IT WAS TOO STRONG, so it is corrected here rather than left standing: equal CPU
// descriptor handles prove the same descriptor SLOT, not the same resource. The engine is free to rewrite
// that slot to point at a different texture for each graph, and this probe cannot tell the two apart --
// resolving it would need CreateDepthStencilView intercepted the way CreateRenderTargetView already is.
//
// AND THE CAPTURE SETTLED IT: ONE SLOT, TWO TEXTURES. Nsight names resources directly in barriers, and the
// two cascade runs in a frame (318 draws each, events 19604..21387 and 46338..48113) clear the very same DSV
// -- DescriptorHeap_58 @ 22, which is what this probe saw -- while barriering DIFFERENT resources:
//
//     run 0 (first view):  Resource_48841 x3   + Resource_5159 x1
//     run 1 (second run):  Resource_4843  x3   + Resource_5159 x1
//
// So the engine rewrites that descriptor per graph and each view rasterises into its OWN atlas. There was
// never anything shared to hand over, which is exactly why all three reuse attempts produced artefacts (see
// CyberpunkVR_CascadeSaveMain in ViewReuse.cpp): each one made a view sample a texture that nothing in its
// own graph had written.
//
// The CONTENT of the two is still duplicated -- identical record, identical casters -- so the work is
// redundant even though the texture is not shared. Saving it would mean copying one atlas into the other, or
// repointing the sampling descriptor, not skipping a pass.
//
// Default 0: it has answered, and its answer needed the capture to be read correctly.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_CascRtProbe = 0;
namespace {
struct CascRt { uint64_t dsv[2]; uint64_t hits[2]; };
CascRt g_cascrt[8] = {};

void cascrt_note(uint64_t dsv, int32_t idx, bool vrcam) {
    if (idx < 0 || idx >= 8) return;
    const int v = vrcam ? 1 : 0;
    g_cascrt[idx].dsv[v] = dsv;
    ++g_cascrt[idx].hits[v];
}

void cascrt_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    char line[600];
    int used = 0;
    line[0] = 0;
    for (int i = 0; i < 8; ++i) {
        if (!g_cascrt[i].hits[0] && !g_cascrt[i].hits[1]) continue;
        const bool both = g_cascrt[i].hits[0] && g_cascrt[i].hits[1];
        if (used < static_cast<int>(sizeof(line)) - 90)
            used += snprintf(line + used, sizeof(line) - used,
                             "casc%d M=%llX(%llu) V=%llX(%llu) %s  ", i,
                             (unsigned long long)g_cascrt[i].dsv[0],
                             (unsigned long long)g_cascrt[i].hits[0],
                             (unsigned long long)g_cascrt[i].dsv[1],
                             (unsigned long long)g_cascrt[i].hits[1],
                             !both ? "ONE-SIDED"
                                   : (g_cascrt[i].dsv[0] == g_cascrt[i].dsv[1] ? "SAME" : "differ"));
    }
    log("[cascrt] depth target bound by the cascade pass, per view and index: %s",
        used ? line : "(none seen)");
}
}  // namespace

void STDMETHODCALLTYPE hk_OMSetRenderTargets(
        ID3D12GraphicsCommandList* self, UINT count,
        const D3D12_CPU_DESCRIPTOR_HANDLE* handles, BOOL contiguous,
        const D3D12_CPU_DESCRIPTOR_HANDLE* depth) {
    PFN_OMSetRenderTargets original = command_list_original_om(self);
    if (!original) return;
    // The cascade pass binds a DEPTH target and usually no colour one, so this is deliberately ahead of the
    // `count >= 1` gate below -- inside it the probe would never fire.
    if (CyberpunkVR_CascRtProbe && g_exe_base && depth) {
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        const uintptr_t work = t_current_node_work;
        if (work > base && static_cast<uint32_t>(work - base) == CASCADE_NODE_RVA) {
            uint64_t ptr = 0;
            __try { ptr = static_cast<uint64_t>(depth->ptr); }
            __except (EXCEPTION_EXECUTE_HANDLER) { ptr = 0; }
            if (ptr) {
                cascrt_note(ptr, t_cascade_idx, t_vrcam_node_active);
                cascrt_report();
            }
        }
    }
    // Which frame-graph node is binding, and what it is binding. Deliberately NOT gated on any
    // one probe: the HUD surface is identified here, and that is a feature, not diagnostics.
    if (g_exe_base && handles && count >= 1) {
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        const uintptr_t work = t_current_node_work;
        const uint32_t rva = (work > base) ? static_cast<uint32_t>(work - base) : 0;
        ID3D12Resource* bound = nullptr;
        const bool hud_node = CyberpunkVR_HudByNode &&
            (rva == CyberpunkVR_HudNodeRva || rva == CyberpunkVR_HudNodeRva2);
        if (CyberpunkVR_RtMapProbe || CyberpunkVR_HudNodeProbe || hud_node) {
            __try { bound = rtv_resource_lookup(handles[0].ptr); }
            __except (EXCEPTION_EXECUTE_HANDLER) { bound = nullptr; }
        }
        if (CyberpunkVR_RtMapProbe && bound) rtmap_note(bound, t_vrcam_node_active);
        if (hud_node) {
            g_hud_node_binds.fetch_add(1, std::memory_order_relaxed);
            if (!bound) g_hud_node_unresolved.fetch_add(1, std::memory_order_relaxed);
        }
        if (hud_node && bound) hud_adopt_by_node(bound);
        if (CyberpunkVR_HudNodeProbe && bound) hud_node_note(bound, t_vrcam_node_active);
    }
    // Track the primary bound RT dims for THIS recording thread, so the RSSetViewports/ScissorRects
    // hooks can detect the crop pass (render-res viewport on an OUTPUT-size RT) reliably -- both run
    // consecutively on the SAME thread, no dependency on which thread ran the DLSS eval.
    if (handles && count >= 1) {
        __try {
            uint32_t rw = 0, rh = 0;
            if (rtv_dim_lookup(handles[0].ptr, &rw, &rh)) {
                t_cur_rt_w = rw; t_cur_rt_h = rh;
            } else { t_cur_rt_w = 0; t_cur_rt_h = 0; }
        } __except (EXCEPTION_EXECUTE_HANDLER) { t_cur_rt_w = 0; t_cur_rt_h = 0; }
    }
    // Redirect the vrcam-final RTV to our own committed target: the engine then renders
    // the final into a stable, never-aliased resource we control (kills the shared-heap
    // race where the mirror read main's content). Only inside the ctx-keyed vrcam
    // RenderFinal2D node (view-aware) and only for the vrcam-dims target (mirror_find_bound_rtv
    // returns only dims-filtered candidates).
    // (The HUD snapshot used to be triggered here, at the unbind of the HUD's mip-0 target. That
    //  is too early: the glow mips do not exist yet. It now hangs off the barrier that releases
    //  the last mip -- see hk_ResourceBarrier.)

    D3D12_CPU_DESCRIPTOR_HANDLE sub[8];
    const D3D12_CPU_DESCRIPTOR_HANDLE* use = handles;
    BOOL use_contig = contiguous;
    bool subbed = false;
    if (CyberpunkVR_VrcamOwnTarget && t_mirror_copy_node_active && handles && count &&
            count <= 8 && g_game_device) {
        __try {
            const UINT inc = g_game_device->GetDescriptorHandleIncrementSize(
                D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
            for (UINT i = 0; i < count; ++i) {
                sub[i].ptr = contiguous ? handles[0].ptr + (SIZE_T)inc * i
                                        : handles[i].ptr;
                DXGI_FORMAT vf = DXGI_FORMAT_UNKNOWN;
                ID3D12Resource* res = mirror_find_bound_rtv(sub[i].ptr, &vf);
                if (res) {
                    D3D12_CPU_DESCRIPTOR_HANDLE own =
                        mirror_ensure_own_target_rtv(res, vf);
                    if (own.ptr) { sub[i] = own; subbed = true; }
                }
            }
            if (subbed) { use = sub; use_contig = FALSE; }
        } __except (EXCEPTION_EXECUTE_HANDLER) {
            subbed = false; use = handles; use_contig = contiguous;
        }
    }
    original(self, count, use, use_contig, depth);
    if (subbed) {
        ++CyberpunkVR_DebugOwnTargetSubs;
        if (g_own_target)
            g_captured_vrcam_res.store(g_own_target, std::memory_order_release);
    }
    // 2-MRT probe: the vrcam post-DLSS pass binds {transient RT0, persistent RT1} where
    // RT1's descriptor PING-PONGS A/B/A across frames (Nsight 3-frame diff; main's pair
    // is constant). RT0 resolves via the dims-filtered candidates (vrcam-res only, so
    // main's 2-RT pass self-filters out). Log the first occurrences to identify the
    // ping-pong pair in the live session; count all hits.
    bool is_2rt_bind = false;
    if (count == 2 && handles && g_game_device) {
        const UINT inc2 = g_game_device->GetDescriptorHandleIncrementSize(
            D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
        const SIZE_T h0 = handles[0].ptr;
        const SIZE_T h1 = contiguous ? handles[0].ptr + inc2 : handles[1].ptr;
        DXGI_FORMAT vf0 = DXGI_FORMAT_UNKNOWN;
        ID3D12Resource* rt0 = mirror_find_bound_rtv(h0, &vf0);
        if (rt0) {
            is_2rt_bind = true;
            InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
                &CyberpunkVR_Debug2RtBinds));
            // Tonemap identification: RT1 changed for a known RT0 => ping-pong pass.
            int slot = -1;
            for (int i = 0; i < 4; ++i) {
                const SIZE_T seen =
                    g_2rt_seen_h0[i].load(std::memory_order_acquire);
                if (seen == h0) { slot = i; break; }
                if (!seen) {
                    SIZE_T want = 0;
                    if (g_2rt_seen_h0[i].compare_exchange_strong(want, h0)) {
                        g_2rt_seen_h1[i].store(h1, std::memory_order_release);
                        slot = -2;      // just inserted; no history yet
                    } else if (g_2rt_seen_h0[i].load(std::memory_order_acquire)
                               == h0) {
                        slot = i;
                    }
                    if (slot != -1) break;
                }
            }
            if (slot >= 0 &&
                g_2rt_seen_h1[slot].exchange(h1, std::memory_order_acq_rel) != h1) {
                SIZE_T want = 0;
                g_tonemap_h0.compare_exchange_strong(want, h0);   // identify tonemap pass ([2rt] log removed)
            }
            // CB-probe window (diagnostic) still keyed on the ping-pong h0.
            if (g_tonemap_h0.load(std::memory_order_relaxed) == h0) {
                t_in_vrcam_2rt = true;
                t_2rt_cb_armed = true;
            }
            // Tonemap RT0 capture keyed on WORK-RVA 0x768510 (STABLE across frames),
            // NOT the per-frame descriptor handle h0 (which stopped matching after ~16
            // frames and froze the snapshot). RT0 = the tonemapped color OUTPUT (proven
            // by the full reverse: DLSS writes raw linear to post-color at an early gen;
            // tonemap writes the correct dark result; RTT-Final2D reads the early gen).
            // The node epilogue snapshots this into committed g_stable_tex.
            // ONE WRITER PER SLOT, and it stands down only once the finished-frame source has
            // actually delivered. Both writers alive at once made the snapshot's source desc alternate
            // every frame -- an 8-bit finished frame against this mid-chain R11G11B10 target -- and
            // every alternation abandoned a full-size texture: 745 recreations, then a D3D12MA
            // out-of-memory. Keying the standdown on the landed-copy latch rather than on the key
            // alone keeps the worst case at "behaves exactly as before".
            if ((!CyberpunkVR_FinalGrab ||
                    !g_have_tonemap_source.load(std::memory_order_acquire)) &&
                    t_current_node_work ==
                    reinterpret_cast<uintptr_t>(g_exe_base) + TONEMAP_WORK_RVA) {
                t_tm_rt0 = rt0;
                t_tm_from_final = false;   // this slot was armed by the tonemap, not the finished frame
                t_tm_rt0_list = self;
                t_tm_rt0_state = (uint32_t)D3D12_RESOURCE_STATE_RENDER_TARGET;
                t_tm_rt0_state_seen = false;   // a seed, not an observation
                t_tm_consumed = false;   // arm the epilogue snapshot
            }
            // ([2rt] per-bind diagnostic logging removed)
        }
    }
    if (!is_2rt_bind) t_in_vrcam_2rt = false;   // any other bind closes the window
    if (!t_mirror_copy_node_active || !handles || !count) return;
    const UINT increment = g_game_device
        ? g_game_device->GetDescriptorHandleIncrementSize(
            D3D12_DESCRIPTOR_HEAP_TYPE_RTV) : 0;
    for (UINT i = 0; i < count; ++i) {
        const SIZE_T handle = contiguous
            ? handles[0].ptr + static_cast<SIZE_T>(increment) * i
            : handles[i].ptr;
        DXGI_FORMAT view_format = DXGI_FORMAT_UNKNOWN;
        ID3D12Resource* resource =
            mirror_find_bound_rtv(handle, &view_format);
        // ---- RTV-pick diagnostic ---------------------------------------------------
        // Which render target the mirror latches decides what the VRCAM eye shows, and
        // the pick is dims-filtered: it assumes nothing else in the frame is the VRCAM
        // size. With the MAIN resolution override that assumption is no longer safe --
        // MAIN is forced to the SAME square size as VRCAM. On top of that this loop
        // keeps the LAST matching target of a multi-RT bind, so a 2-MRT vrcam pass
        // whose second target now also passes the filter would latch the wrong one.
        // Log the first binds so the actual pick is a fact, not a guess.
        if (CyberpunkVR_DebugRtvPickLog > 0) {
            --CyberpunkVR_DebugRtvPickLog;
            D3D12_RESOURCE_DESC rd{};
            // "not-a-candidate" on its own does not say WHY, and why is the whole question once
            // the node has started binding something we do not know. The broad RTV->resource map
            // is filled by the same CreateRenderTargetView hook with no format or size filter at
            // all, so it answers it: present there but missing from the candidate table means
            // mirror_register_rtv REFUSED it, and the dims and format printed are the ones it
            // refused. Missing from both means we never saw that descriptor created.
            //
            // Reported through a SEPARATE local. `resource` is the pick this loop acts on; writing
            // the diagnostic's answer back into it would turn a log line into a behaviour change.
            ID3D12Resource* shown = resource;
            const char* via = "";
            if (!shown) {
                shown = rtv_resource_lookup(handle);
                via = shown ? " [refused by the candidate filter]"
                            : " [descriptor never seen created]";
            }
            const bool got = shown && mirror_get_resource_desc(shown, &rd);
            log("[rtvpick] bind i=%u/%u handle=%p -> res=%p %s%llux%u fmt=%u view=%u %s%s",
                i, count, (void*)handle, (void*)shown,
                got ? "" : "(no desc) ",
                got ? (unsigned long long)rd.Width : 0ull,
                got ? rd.Height : 0u,
                got ? (unsigned)rd.Format : 0u,
                (unsigned)view_format,
                resource ? "MATCH-latched" : "not-a-candidate",
                via);
        }
        if (resource) {
            ++CyberpunkVR_DebugMirrorRtvHits;
            t_mirror_copy_rtv = resource;
            t_mirror_copy_rtv_format = view_format;
            // Engine activated this target with ALIASING + ->RENDER_TARGET + Discard
            // right before binding it (ev95193<ev95195); track further transitions
            // in hk_ResourceBarrier so the inline copy uses the exact current state.
            t_mirror_copy_list = self;
            t_mirror_src_state = (uint32_t)D3D12_RESOURCE_STATE_RENDER_TARGET;
            t_mirror_src_state_seen = false;   // a seed, not an observation
            // This command list runs the vrcam CopyToTexture blit -> trigger the
            // 11on12 copy only when the game SUBMITS it (correct frame contents).
            g_mirror_pending_list.store(self, std::memory_order_release);
        }
    }
}

// A MILLISECOND CLOCK THAT CAN SEE A FRAME. GetTickCount64 has ~15.6 ms granularity, and the second
// eye's staleness is interesting at 14 ms and below -- so measured with it, the age came back as exactly
// 0, 16 or 31 ms and the reported max sat pinned at 16.0 in almost every window. That is one tick, not a
// measurement. Every frame-scale conclusion drawn from it would have been an artefact of the clock.
//
// QPC instead, still in milliseconds so CyberpunkVR_StereoEyeMaxAgeMs keeps its units and its meaning.
// MICROSECONDS, alongside the milliseconds the overlay and the gate use. The ms value cannot
// express a frame; this one can.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugVrcamEyeAgeUs = 0;

static double StableNowMs() {
    static LARGE_INTEGER s_freq = {};
    if (s_freq.QuadPart == 0) QueryPerformanceFrequency(&s_freq);
    if (s_freq.QuadPart == 0) return 0.0;
    LARGE_INTEGER t{};
    QueryPerformanceCounter(&t);
    return (double)t.QuadPart * 1000.0 / (double)s_freq.QuadPart;
}

static bool mirror_stage_desc_matches(const D3D12_RESOURCE_DESC& a,
                                      const D3D12_RESOURCE_DESC& b) {
    return a.Dimension == b.Dimension && a.Width == b.Width &&
        a.Height == b.Height && a.DepthOrArraySize == b.DepthOrArraySize &&
        a.MipLevels == b.MipLevels && a.Format == b.Format &&
        a.SampleDesc.Count == b.SampleDesc.Count &&
        a.SampleDesc.Quality == b.SampleDesc.Quality;
}


// Record, INTO THE ENGINE'S OWN command list, a copy of the freshly written vrcam
// final into our committed g_stable_tex. Called at the end of the vrcam RenderFinal2D
// node: the composite draw is already recorded, no later pass is recorded yet, so in
// queue order the copy executes after the final write and BEFORE any ALIASING barrier
// re-purposes the transient's heap memory (the proven bright/dark root). No SEH here:
// same risk class as the existing raw-vtable append path (uses C++ locks => C2712).
// A TYPELESS SNAPSHOT CANNOT BE VIEWED, AND THAT IS THE DEVICE REMOVAL.
//
// The snapshot below inherits the SOURCE's desc, and the finished frame the second view's composition
// writes is R8G8B8A8_TYPELESS. ColorBlit then does `srv.Format = srcColor->GetDesc().Format` and calls
// CreateShaderResourceView with a typeless format, which D3D12 does not allow. Measured, with
// xr_final_grab armed: the log reads `[stable] committed vrcam snapshot 3072x3072 fmt=27` -- 27 is
// R8G8B8A8_TYPELESS, where the old mid-chain source read fmt=26 (R11G11B10_FLOAT) and viewed cleanly --
// and the session ends `presentHr=0x887A0005 reason=0x887A0001` with `PageFaultVA=0`: DEVICE_REMOVED
// for INVALID_CALL, a badly formed call rather than a bad address. The note beside nd.Flags further
// down already records this project losing a device to "a typeless RTV elsewhere"; this is the same
// class of mistake one resource along.
//
// So the snapshot is typed. CopyResource stays legal because a typed member and its TYPELESS parent
// are in one copy family, and every view over the snapshot -- ColorBlit's SRV, the desktop mirror's
// shared surface -- becomes legal at once.
//
// WHICH member of the 8-bit family only the picture can settle, so it is a live key rather than a
// rebuild:
//   1 (ships)  _UNORM_SRGB. The finished frame carries sRGB-ENCODED bytes, so the sampler decodes to
//              linear and ColorBlit's own _UNORM_SRGB render target encodes back -- a round trip. That
//              also cancels the second encode, which is what shows as overexposure.
//   0          _UNORM. The bytes are sampled raw; correct if that frame turns out to hold linear.
// A float source is already typed and is handed back untouched, so nothing changes without the grab.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_StableSrgbView = 1;

static DXGI_FORMAT stable_typed_format(DXGI_FORMAT f) {
    switch (f) {
        case DXGI_FORMAT_R8G8B8A8_TYPELESS:
            return CyberpunkVR_StableSrgbView ? DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
                                              : DXGI_FORMAT_R8G8B8A8_UNORM;
        case DXGI_FORMAT_B8G8R8A8_TYPELESS:
            return CyberpunkVR_StableSrgbView ? DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
                                              : DXGI_FORMAT_B8G8R8A8_UNORM;
        case DXGI_FORMAT_R10G10B10A2_TYPELESS:  return DXGI_FORMAT_R10G10B10A2_UNORM;
        case DXGI_FORMAT_R16G16B16A16_TYPELESS: return DXGI_FORMAT_R16G16B16A16_FLOAT;
        case DXGI_FORMAT_R32G32B32A32_TYPELESS: return DXGI_FORMAT_R32G32B32A32_FLOAT;
        default: return f;
    }
}

 void mirror_stable_inline_copy(ID3D12GraphicsCommandList* list,
        ID3D12Resource* src, uint32_t src_state) {
    if (!g_game_device || !list || !src) return;
    const CommandListVtableHook* e = command_list_hook_entry(list);
    D3D12_RESOURCE_DESC d{};
    if (!e || !e->barrier_call || !e->copyres || !mirror_get_resource_desc(src, &d)) {
        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
            &CyberpunkVR_DebugStableSkips));
        return;
    }
    ID3D12Resource* stable = nullptr;
    {
        std::lock_guard<std::mutex> lk(g_stable_mtx);
        if (g_stable_tex && !mirror_stage_desc_matches(g_stable_desc, d)) {
            // Resolution changed: old snapshot may still be referenced by an in-flight
            // deferred copy -> intentionally leak it (rare, dev-time only) and recreate.
            g_stable_tex = nullptr;
        }
        if (!g_stable_tex) {
            D3D12_HEAP_PROPERTIES hp{};
            hp.Type = D3D12_HEAP_TYPE_DEFAULT;
            hp.CreationNodeMask = 1;
            hp.VisibleNodeMask = 1;
            D3D12_RESOURCE_DESC nd = d;
            nd.Format = stable_typed_format(d.Format);   // see the note above this function
            // Plain copy target -- no ALLOW_SIMULTANEOUS_ACCESS.
            //
            // I added that flag chasing the first device hang, before the real cause turned
            // out to be a typeless RTV elsewhere. It does not belong here and it is not free:
            // d12_mirror_ensure() builds the mirror texture from THIS desc, so the flag was
            // inherited by the surface the mirror shares with D3D11, and enabling the mirror
            // died with DXGI_ERROR_ACCESS_DENIED (0x887A002B). The working build never set it.
            nd.Flags = D3D12_RESOURCE_FLAG_NONE;
            ID3D12Resource* tex = nullptr;
            if (FAILED(g_game_device->CreateCommittedResource(&hp,
                    D3D12_HEAP_FLAG_NONE, &nd, D3D12_RESOURCE_STATE_COMMON,
                    nullptr, IID_PPV_ARGS(&tex))) || !tex) {
                InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
                    &CyberpunkVR_DebugStableSkips));
                return;
            }
            tex->SetName(L"CyberpunkVR_VrcamStable");
            g_stable_tex = tex;
            g_stable_desc = d;
            log("[stable] committed vrcam snapshot=%p %llux%u fmt=%u (source fmt=%u)", tex,
                (unsigned long long)d.Width, d.Height, (unsigned)nd.Format,
                (unsigned)d.Format);
        }
        stable = g_stable_tex;
    }
    // src: current state -> COPY_SOURCE (skip if already there); stable: COMMON ->
    // COPY_DEST; copy; restore BOTH so the engine's own state tracking stays intact.
    D3D12_RESOURCE_BARRIER b[2]{};
    UINT nb = 0;
    if (src_state != (uint32_t)D3D12_RESOURCE_STATE_COPY_SOURCE) {
        b[nb].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b[nb].Transition.pResource = src;
        b[nb].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        b[nb].Transition.StateBefore = (D3D12_RESOURCE_STATES)src_state;
        b[nb].Transition.StateAfter  = D3D12_RESOURCE_STATE_COPY_SOURCE;
        ++nb;
    }
    b[nb].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b[nb].Transition.pResource = stable;
    b[nb].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    b[nb].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
    b[nb].Transition.StateAfter  = D3D12_RESOURCE_STATE_COPY_DEST;
    ++nb;
    e->barrier_call(list, nb, b);
    e->copyres(list, stable, src);
    nb = 0;
    if (src_state != (uint32_t)D3D12_RESOURCE_STATE_COPY_SOURCE) {
        b[nb].Transition.pResource = src;
        b[nb].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
        b[nb].Transition.StateAfter  = (D3D12_RESOURCE_STATES)src_state;
        ++nb;
    }
    b[nb].Transition.pResource = stable;
    b[nb].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    b[nb].Transition.StateAfter  = D3D12_RESOURCE_STATE_COMMON;
    ++nb;
    e->barrier_call(list, nb, b);
    g_stable_fresh.store(true, std::memory_order_release);
    // TWO STAMPS, AND THE REASON IS A REGRESSION I CAUSED. g_stable_tick is MILLISECONDS from
    // GetTickCount64 and always was; three readers compare it against GetTickCount64. I changed it to
    // QPC microseconds to get frame-scale resolution for the age measurement, and the HUD-snapshot
    // liveness test at the bottom of this file then compared microseconds-since-QPC-epoch against
    // milliseconds-since-boot. Different unit AND different epoch, so `live` went false and the HUD
    // silently vanished from the second eye.
    //
    // So the coarse stamp keeps its meaning for the gates, and the fine one exists beside it.
    g_stable_tick.store(GetTickCount64(), std::memory_order_release);
    g_stable_tick_us.store((uint64_t)(StableNowMs() * 1000.0), std::memory_order_release);
    CyberpunkVR_DebugStableSrcState = src_state;
    InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
        &CyberpunkVR_DebugStableCopies));
}

// ---- VRCAM eye source for the OpenXR stereo submit ----------------------------------
// Hands the submit path the VRCAM final colour for eye 1. This is the SAME resource the
// desktop mirror reads: written inside the ENGINE'S OWN command list at the end of the
// vrcam RenderFinal2D node, so by the time Present runs the copy is already ordered
// behind the final write and the resource rests in COMMON -- no barrier or fence wait
// is needed on the caller's side, and nothing can alias it (it is committed, not a
// transient from the frame-graph heap).
//
// Format note for the submit path: this carries the RenderFinal2D output format, which
// is R11G11B10_FLOAT holding LINEAR values -- NOT the sRGB-encoded R8G8B8A8_UNORM bytes
// the MAIN backbuffer holds. Eye 1 therefore needs the encode pass, not a raw copy.
// Returns null until the VRCAM node has produced its first frame (component disabled,
// or the first frames after load).
extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetVrcamEyeTexture() {
    if (CyberpunkVR_VrcamOwnTarget) {
        std::lock_guard<std::mutex> lk(g_own_target_mtx);
        if (g_own_target) return g_own_target;
    }
    if (!g_stable_fresh.load(std::memory_order_acquire)) return nullptr;
    std::lock_guard<std::mutex> lk(g_stable_mtx);
    return g_stable_tex;
}

// Same resource, but null once the second view has gone quiet -- the form the OpenXR submit
// wants. Existence is not enough there: handing eye 1 a snapshot the second view stopped
// updating pins one eye to a still image while the other keeps moving, which reads far worse
// than dropping to mono. Age also drives the overlay's status line.

extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetVrcamEyeTextureFresh() {
    const uint64_t last = g_stable_tick.load(std::memory_order_acquire);
    if (!last) { CyberpunkVR_DebugVrcamEyeAgeMs = 0xFFFFFFFFu; return nullptr; }
    // THE GATE RUNS ON THE COARSE CLOCK, exactly as it did before: same unit, same epoch, same
    // behaviour. The REPORTED age runs on the fine one, which is the only thing that needed changing --
    // GetTickCount64's 15.6 ms granularity cannot express a 14 ms staleness, and reported max 16.0 ms
    // in every window because that is one tick.
    const uint64_t age = GetTickCount64() - last;
    const uint64_t lastUs = g_stable_tick_us.load(std::memory_order_acquire);
    const uint64_t nowUs = (uint64_t)(StableNowMs() * 1000.0);
    const uint64_t ageUs = (lastUs && nowUs > lastUs) ? nowUs - lastUs : 0;
    CyberpunkVR_DebugVrcamEyeAgeMs =
        age > 0xFFFFFFFEull ? 0xFFFFFFFEu : static_cast<uint32_t>(age);
    CyberpunkVR_DebugVrcamEyeAgeUs = ageUs > 0xFFFFFFFEull ? 0xFFFFFFFEu : (uint32_t)ageUs;
    if (age > CyberpunkVR_StereoEyeMaxAgeMs) {
        // THIS RETURN IS THE MONO. Say why, once, with the tally that separates the four ways the
        // snapshot can stop -- and separates all of them from "the second view stopped rendering",
        // which is a different failure with the same symptom.
        //
        //   nodeHits   the vrcam CopyToTexture node still runs      (0 => the node is gone)
        //   noRtv      it ran but bound no output we recognise      (a rebuilt graph's new target)
        //   noList     output found, no hooked command list         (invisible to the publish
        //                                                            watchdog -- it needs neither)
        //   copies     the snapshot was attempted                   (climbing => look at skips)
        //   skips      attempted and refused inside the copy
        // ANSWERED, and now one level deeper. Measured: nodeHits climbs at frame rate while noRtv
        // climbs at exactly the same rate and copies stands still -- the node runs every frame and
        // binds an output that mirror_find_bound_rtv does not recognise. So the break is the RTV
        // CANDIDATE TABLE, and there are only two ways it can fail: the new target's RTV was never
        // registered (the gate in mirror_register_rtv refused it), or it was registered and then
        // EVICTED, the 512-slot ring having wrapped -- which is the third time an array in this
        // file would have quietly filled up. regs/evicts/live separate those two outright.
        static uint64_t s_lastSaidMs = 0;
        const uint64_t now = GetTickCount64();
        if (now - s_lastSaidMs > 5000) {
            s_lastSaidMs = now;
            log("[stereo-eye] stale by %llu ms (limit %u) -> submitting MONO. "
                "nodeHits=%llu noRtv=%llu noList=%llu copies=%llu skips=%llu attempted=%llu | "
                "rtv table: live=%u/%u regs=%llu evicts=%llu",
                (unsigned long long)age, CyberpunkVR_StereoEyeMaxAgeMs,
                (unsigned long long)g_eye_node_hits.load(std::memory_order_relaxed),
                (unsigned long long)g_eye_no_rtv.load(std::memory_order_relaxed),
                (unsigned long long)g_eye_no_list.load(std::memory_order_relaxed),
                (unsigned long long)CyberpunkVR_DebugStableCopies,
                (unsigned long long)CyberpunkVR_DebugStableSkips,
                (unsigned long long)g_eye_copy_calls.load(std::memory_order_relaxed),
                g_mirror_rtv_candidate_count.load(std::memory_order_relaxed),
                (unsigned)g_mirror_rtv_candidates.size(),
                (unsigned long long)CyberpunkVR_DebugMirrorRtvRegs,
                (unsigned long long)CyberpunkVR_DebugMirrorRtvEvicts);
            // And show the actual binds. [rtvpick] burned its budget down during startup, when
            // everything worked; re-arm a short burst here so the next few frames print the
            // handles the node is binding NOW and what, if anything, they resolve to.
            if (CyberpunkVR_DebugRtvPickLog <= 0) CyberpunkVR_DebugRtvPickLog = 6;
        }
        return nullptr;
    }
    return CyberpunkVR_GetVrcamEyeTexture();
}

// Append (into the game's OWN list) a copy dtex -> g_d12_mtex. Called from the
// barrier hook at the dtex's RENDER_TARGET->'after' transition, so dtex holds the
// freshly rendered frame and is in 'after'; bracket the copy with matching states.
void d12_append_mirror_copy(const CommandListVtableHook* e,
        ID3D12GraphicsCommandList* list, ID3D12Resource* dtex,
        D3D12_RESOURCE_STATES after) {
    if (!e || !e->copyres || !e->barrier_call || !dtex || !g_d12_mtex) return;
    D3D12_RESOURCE_BARRIER b[2]{};
    b[0].Type = b[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b[0].Transition.pResource = dtex;
    b[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    b[0].Transition.StateBefore = after;
    b[0].Transition.StateAfter  = D3D12_RESOURCE_STATE_COPY_SOURCE;
    b[1].Transition.pResource = g_d12_mtex;
    b[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    b[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
    b[1].Transition.StateAfter  = D3D12_RESOURCE_STATE_COPY_DEST;
    e->barrier_call(list, 2, b);
    e->copyres(list, g_d12_mtex, dtex);
    b[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    b[0].Transition.StateAfter  = after;
    b[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    b[1].Transition.StateAfter  = D3D12_RESOURCE_STATE_COMMON;
    e->barrier_call(list, 2, b);
    g_mirror_pending_list.store(list, std::memory_order_release);
}

// ---- and the state tracking every copy above depends on ---------------------------------------
// The [indarg] watch that used to sit here is DELETED. It recorded every barrier moving a
// resource into or out of INDIRECT_ARGUMENT, for the 0x88 driver fault, and it answered its
// question on 2026-08-29: the second view issues 67452 such transitions to MAIN's 80986, on the
// same nodes, with no node exclusive to either -- so the hypothesis it was built to test was
// wrong, and it had nothing left to say.
//
// What it did instead was cost a session. Left armed through the ini's write-back, it printed on
// every change of node or view from inside ResourceBarrier -- 36452 lines in 203 seconds, and in
// a menu, where the pattern never settles, continuously. The frame rate collapsed and the run
// ended in DXGI_ERROR_DEVICE_REMOVED: the CPU stopped feeding the GPU and submission ran past the
// driver's timeout. That is the exact mechanism this project already had written down about
// always-on probes, paid for a second time.
// ==== RESTORED FROM THE 0.1.6 RENDER (7dd29c36) ============================================
//
// The other half of the composition lend: with xr_comp_lend_set armed the second view runs
// DrawComposition, CompositionPostProcess and DrawHUD (measured in that session's [wide] census:
// CompositionPostProcess M=8110 V=742), and this is what takes the frame those produce.

// ---- THE SECOND EYE'S FINISHED FRAME -----------------------------------------------------------
//
// The port used to submit a texture remembered at ApplyTXAA -- mid-chain, R11G11B10_FLOAT, with a THIRD
// of the frame at exactly zero where the finished image has none (measured over the whole 3072x3072
// picture). The finished image is a full-size R8G8B8A8_TYPELESS written by a COMPUTE dispatch through a
// UAV inside the second view's CompositionPostProcess, so no render-target substitution can reach it and
// no UAV->SRV publish barrier exists for it (nothing downstream reads it in that view).
//
// What DOES exist is the engine's own ACQUIRE barrier on that texture, which the census read as
// `DrawHUD 3072/f27 RENDER_TARGET -> UNORDERED_ACCESS` in the second view's window. So the barrier hook
// learns the pointer AND the state there -- no guessing, no patch to the command stream -- and the
// epilogue of CompositionPostProcess does the copy, which is the first moment the contents are final and
// still before anything aliases the transient's heap.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_FinalGrab = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFinalGrabs = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFinalArms = 0;

// CompositionPostProcess: the one node the finished frame is acquired, written and released inside.
static constexpr uint32_t COMPOSITION_NODE_RVA = 0x1F8928;

// The second view's finished-frame target, learned from the engine's own acquire barrier in that node
// and copied when the same node releases it.
//
// THREAD-LOCAL, and that is not a micro-optimisation. These were globals, and the engine records nodes
// on several threads at once (redDispatcher2 and redDispatcher12 both appear in one frame's capture), so
// a global handed the epilogue a command list belonging to ANOTHER thread -- or a list from the previous
// frame, by then closed and recycled. Appending a copy to either is a badly formed call, and the device
// went out with DXGI_ERROR_INVALID_CALL. All three are written and read on the one thread that records
// this view's node, so thread_local is both correct and stricter.
// Defined in SyncStereo.cpp beside the other per-thread render state. t_final_writes counts the
// draws/dispatches/ExecuteIndirects recorded on that list since the acquire: the copy waits for at least
// one, because a barrier batch arriving before any write would copy an undefined surface.
extern thread_local ID3D12Resource*            t_final_res;
extern thread_local uint32_t                   t_final_state;
extern thread_local ID3D12GraphicsCommandList* t_final_list;
extern thread_local uint32_t                   t_final_writes;

// ---- the finished frame reaches the eye at the NODE EPILOGUE, exactly as the old capture does ----
//
// The mirror path that has fed this eye for months works like this: learn the output resource inside
// the node that writes it, and record the copy in THAT NODE'S EPILOGUE -- the work-fn has returned, so
// every write is in the list; the list is still open; the resource is not aliased yet. It never misses
// a frame.
//
// Copying from inside the barrier hook instead landed on one frame in five, whichever scope was tried
// (the node work-fn, the live view flag, a set of command lists): none of them is reliably true at the
// instant the engine retires this particular resource. So the barrier hook only ARMS the slot, and this
// is called from the second view's node epilogue in NodeDispatch.cpp.
void final_frame_copy_now() {
    if (!t_final_res || !t_final_state || !t_final_list) return;
    if (CyberpunkVR_StableCopy && stereo_eye_capture_wanted()) {
        mirror_stable_inline_copy(t_final_list, t_final_res, t_final_state);
        // The latch that retires the older sources. One-way on purpose: once the finished frame has
        // delivered once, the tonemap-RT0 capture stops arming and the mirror copy stands down, so the
        // two can never again alternate formats into the same snapshot texture.
        g_have_tonemap_source.store(true, std::memory_order_release);
        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugFinalGrabs));
    }
    t_final_res = nullptr;
    t_final_state = 0;
    t_final_list = nullptr;
}

void STDMETHODCALLTYPE hk_ResourceBarrier(ID3D12GraphicsCommandList* self,
        UINT count, const D3D12_RESOURCE_BARRIER* barriers) {
    const CommandListVtableHook* e = command_list_hook_entry(self);
    PFN_ResourceBarrier orig = e ? e->barrier_original : nullptr;
    if (orig) orig(self, count, barriers);
    // Gated on the same demand as the snapshot, NOT on the mirror window alone. This hook is
    // where t_mirror_src_state is refined (below): the OM bind seeds it as RENDER_TARGET and
    // every later transition of that target is tracked here, so the inline copy can name the
    // correct "before" state in its own barrier. Leaving it mirror-only meant the eye-capture
    // path would copy with a stale state -- a barrier lie, which the debug layer rejects and
    // the driver may answer with device removal.
    if (!stereo_eye_capture_wanted() || !barriers || !e) return;
    // Placed after that gate rather than at the top of the function as in 0.1.6, because the copy
    // this arms is itself conditional on stereo_eye_capture_wanted(): arming in frames where no eye
    // capture is wanted would leave a transient pointer in the slot with nothing to consume it.
    // ---- THE SECOND VIEW'S FINISHED FRAME: armed here, copied at the node epilogue ---------------
    //
    // The target, read out of renderdoc-capture/frame_3140_aftercompositionfix.rdc in true recording
    // order on the command list the second view records into (RenderDoc's chunk order is per list, so
    // within one list it IS execution order):
    //
    //     84026  aliasing -> 253858, then NON_PIXEL_SRV -> RENDER_TARGET   the graph takes the transient
    //     84028  DiscardResource
    //     84040  RENDER_TARGET -> UNORDERED_ACCESS                        <-- ARMED HERE
    //     84067  ExecuteIndirect                                          the two writes
    //     84072  ExecuteIndirect
    //     84562  UNORDERED_ACCESS -> NON_PIXEL_SRV                        released, heap aliased away
    //
    // THE ACQUIRE, NOT THE RELEASE, and the reason is scope. The acquire sits between the Discard and
    // the writes, so it is provably inside the node that produces this picture; the release arrives in
    // a batch of seven at the tail of the list, long after that node returned. Content is fine either
    // way, because the copy is not recorded here -- it is recorded at the node's epilogue, past both
    // writes.
    //
    // The state the copy names is UNORDERED_ACCESS: it is what the engine itself put the resource in
    // with this very barrier, and it is still that when the node returns. Nothing is guessed.
    //
    // Identification is by exhaustion. Of every full-size candidate in that frame only this one is
    // R8G8B8A8_TYPELESS with both RENDER_TARGET and UNORDERED_ACCESS allowed: the HUD layer is the same
    // 3072x3072 but SRGB and render-target-only, and the half-resolution TYPELESS pair is UAV-only and
    // so cannot make this transition at all.
    if (CyberpunkVR_FinalGrab && barriers && t_vrcam_node_active) {
        for (UINT i = 0; i < count; ++i) {
            const D3D12_RESOURCE_BARRIER& fb = barriers[i];
            if (fb.Type != D3D12_RESOURCE_BARRIER_TYPE_TRANSITION || !fb.Transition.pResource) continue;
            if (fb.Transition.StateBefore != D3D12_RESOURCE_STATE_RENDER_TARGET ||
                    fb.Transition.StateAfter != D3D12_RESOURCE_STATE_UNORDERED_ACCESS) continue;
            D3D12_RESOURCE_DESC fd{};
            if (!mirror_get_resource_desc(fb.Transition.pResource, &fd) ||
                    fd.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D ||
                    fd.Format != DXGI_FORMAT_R8G8B8A8_TYPELESS ||
                    fd.Width < 2048 || fd.Height < 2048) continue;
            if (!(fd.Flags & D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET) ||
                    !(fd.Flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS)) continue;
            t_final_res = fb.Transition.pResource;
            t_final_state = (uint32_t)D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
            t_final_list = self;
            InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugFinalArms));
            // At most once per frame, so the clock read costs nothing. Two numbers, and between them
            // they answer the only question left: armed says the target was found, copied says the
            // epilogue delivered it.
            static uint64_t s_last = 0;
            const uint64_t now = GetTickCount64();
            if (!s_last || now - s_last >= 5000) {
                s_last = now;
                Log("[eyefin] armed=%llu copied=%llu\n",
                    (unsigned long long)CyberpunkVR_DebugFinalArms,
                    (unsigned long long)CyberpunkVR_DebugFinalGrabs);
            }
        }
    }
    // ---- the HUD snapshot, taken once the glow mips exist ------------------------------------
    //
    // This used to fire at the OMSetRenderTargets that unbinds the HUD's mip-0 target, which is
    // BEFORE the engine builds mips 1..4. The composite samples exactly those mips for the glow,
    // so it was reading undefined memory: no quest/map/weapon highlight and thin text. The last
    // mip becomes shader-readable in one barrier, and at that moment ALL five subresources sit in
    // the same PIXEL|NON_PIXEL_SHADER_RESOURCE state -- the one point where a single
    // ALL_SUBRESOURCES transition is honest.
    if (CyberpunkVR_HudToSecondEye && g_hud_res) {
        ID3D12Resource* hud_ready = nullptr;
        const uint64_t tick = g_stable_tick.load(std::memory_order_acquire);
        const uint64_t now = GetTickCount64();
        const uint64_t used = g_hud_consumed_tick.load(std::memory_order_acquire);
        const bool live = tick && (now - tick) < 2000 &&
            (!g_hud_snap_fresh.load(std::memory_order_acquire) ||
             (used && (now - used) < 2000));
        const D3D12_RESOURCE_STATES kShaderRead =
            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE |
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        ID3D12Resource* blur_ready = nullptr;
        if (live) {
            const uint64_t hudW = g_hud_snap_desc.Width ? g_hud_snap_desc.Width : 0;
            for (UINT i = 0; i < count; ++i) {
                const D3D12_RESOURCE_BARRIER& b = barriers[i];
                if (b.Type != D3D12_RESOURCE_BARRIER_TYPE_TRANSITION) continue;
                if (!(b.Transition.StateAfter & D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE))
                    continue;
                if (b.Transition.pResource == g_hud_res) {
                    // The LAST mip: every earlier one was already released to the shader as the
                    // chain was walked, so this is the transition that completes it.
                    if (b.Transition.Subresource ==
                        g_hud_last_mip.load(std::memory_order_acquire)) {
                        hud_ready = g_hud_res;
                    }
                    continue;
                }
                // The blurred-HUD pyramid, on the release of ITS last mip (index 3), where all
                // four subresources are likewise uniform. The engine ping-pongs two of these and
                // reads the one released LAST, so simply letting the later copy win picks it.
                D3D12_RESOURCE_DESC bd{};
                if (b.Transition.Subresource == 3 && hudW &&
                    mirror_get_resource_desc(b.Transition.pResource, &bd) &&
                    hud_blur_signature(bd, hudW)) {
                    blur_ready = b.Transition.pResource;
                }
            }
        }
        // MAIN's finished frame and its scene, both released in the SAME barrier call that
        // retires the HUD. Their descs are not distinctive on their own (several textures share
        // them), but that co-occurrence is: it happens once per frame, at the end of the
        // composite, and nothing else releases the HUD.
        ID3D12Resource* main_out = nullptr;
        ID3D12Resource* main_scene = nullptr;
        D3D12_RESOURCE_STATES main_out_state = D3D12_RESOURCE_STATE_RENDER_TARGET;
        D3D12_RESOURCE_STATES main_scene_state = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        bool hud_retired = false;
        for (UINT i = 0; i < count; ++i) {
            const D3D12_RESOURCE_BARRIER& b = barriers[i];
            if (b.Type == D3D12_RESOURCE_BARRIER_TYPE_TRANSITION &&
                b.Transition.pResource == g_hud_res &&
                b.Transition.StateAfter == D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE) {
                hud_retired = true;
                break;
            }
        }
        if (hud_retired && live) {
            uint64_t w = 0, h = 0;
            {
                std::lock_guard<std::mutex> lk(g_hud_snap_mtx);
                w = g_hud_snap_desc.Width; h = g_hud_snap_desc.Height;
            }
            for (UINT i = 0; i < count && w; ++i) {
                const D3D12_RESOURCE_BARRIER& b = barriers[i];
                if (b.Type != D3D12_RESOURCE_BARRIER_TYPE_TRANSITION) continue;
                D3D12_RESOURCE_DESC rd{};
                if (!b.Transition.pResource ||
                    !mirror_get_resource_desc(b.Transition.pResource, &rd)) continue;
                if (rd.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D ||
                    rd.Width != w || rd.Height != h || rd.MipLevels != 1) continue;
                if (!g_hud_batch_listed) {
                    log("[hud] retire batch: res=%p %llux%u fmt=%u before=%u after=%u",
                        b.Transition.pResource, (unsigned long long)rd.Width, rd.Height,
                        (unsigned)rd.Format, (unsigned)b.Transition.StateBefore,
                        (unsigned)b.Transition.StateAfter);
                }
                // The composite's own output is an 8-bit colour target at output size; the scene
                // it read is the float one. Deliberately not keyed on the transition states --
                // guessing those is what missed it last time.
                if (rd.Format == DXGI_FORMAT_R8G8B8A8_UNORM ||
                    rd.Format == DXGI_FORMAT_R8G8B8A8_TYPELESS ||
                    rd.Format == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB ||
                    rd.Format == DXGI_FORMAT_R10G10B10A2_UNORM) {
                    main_out = b.Transition.pResource;
                    main_out_state = b.Transition.StateAfter;
                } else if (rd.Format == DXGI_FORMAT_R11G11B10_FLOAT) {
                    main_scene = b.Transition.pResource;
                    main_scene_state = b.Transition.StateAfter;
                }
            }
        }

        if (hud_ready)  hud_snapshot_copy(self, hud_ready, kSnapHud, kShaderRead);
        if (blur_ready) hud_snapshot_copy(self, blur_ready, kSnapBlur, kShaderRead);
        // (MAIN's finished frame and its scene are no longer copied: the scene-swap they were
        //  for assumed the composite leaves the scene untouched, which it does not -- it applies
        //  aberration, vignette and grain to the scene itself, so out_main - S_main is far from
        //  zero away from the HUD and MAIN's picture bled over the whole eye.)
        (void)main_out; (void)main_scene; (void)main_out_state; (void)main_scene_state;
        if (hud_retired && !g_hud_batch_listed) {
            g_hud_batch_listed = true;
            log("[hud] retire batch resolved: out=%p scene=%p", main_out, main_scene);
        }
    }
    // Whole-chain capture: during ANY vrcam node, remember every large texture
    // transitioned to a read state (PS 0x80 / NON_PS 0x40), tagged with the node's
    // work RVA. Maps the entire per-frame stage chain in one run.
    if (t_vrcam_node_active) {
        for (UINT i = 0; i < count; ++i) {
            const D3D12_RESOURCE_BARRIER& b = barriers[i];
            if (b.Type != D3D12_RESOURCE_BARRIER_TYPE_TRANSITION ||
                !b.Transition.pResource ||
                !(b.Transition.StateAfter &
                  (D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE |
                   D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE))) {
                continue;
            }
            D3D12_RESOURCE_DESC rd{};
            if (!mirror_get_resource_desc(b.Transition.pResource, &rd) ||
                rd.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D ||
                rd.Width < 1000) {
                continue;
            }
            const uint32_t rva = t_current_node_work
                ? (uint32_t)(t_current_node_work -
                             reinterpret_cast<uintptr_t>(g_exe_base))
                : 0;
            tm_set_push(b.Transition.pResource,
                (uint32_t)b.Transition.StateAfter, (uint32_t)rd.Format, rva);
        }
    }
    // Keep captured pointers' LAST StateAfter fresh (any list, any thread).
    {
        const uint32_t ntm = g_tm_in_n.load(std::memory_order_acquire);
        if (ntm) {
            for (UINT i = 0; i < count; ++i) {
                const D3D12_RESOURCE_BARRIER& b = barriers[i];
                if (b.Type != D3D12_RESOURCE_BARRIER_TYPE_TRANSITION ||
                    !b.Transition.pResource) {
                    continue;
                }
                for (uint32_t k = 0; k < ntm && k < 24; ++k) {
                    if (g_tm_in[k].res.load(std::memory_order_relaxed) ==
                            b.Transition.pResource) {
                        g_tm_in[k].state.store((uint32_t)b.Transition.StateAfter,
                                               std::memory_order_relaxed);
                    }
                }
            }
        }
    }
    // Adapted-exposure accumulator capture: BUFFER W=28 leaving UNORDERED_ACCESS for
    // PS|NPS = the adaptation dispatch just wrote it; attribute by recording view.
    for (UINT i = 0; i < count; ++i) {
        const D3D12_RESOURCE_BARRIER& b = barriers[i];
        if (CyberpunkVR_VisionSnap && b.Type == D3D12_RESOURCE_BARRIER_TYPE_UAV &&
            b.UAV.pResource) {
            D3D12_RESOURCE_DESC vd{};
            if (mirror_get_resource_desc(b.UAV.pResource, &vd) &&
                vision_layer_signature(vd) && vision_matches_last_dispatch(vd)) {
                const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
                const uintptr_t work = t_current_node_work;
                const uint32_t rva = (base && work > base)
                    ? static_cast<uint32_t>(work - base) : 0;
                if (work != t_vision_node) { t_vision_node = work; t_vision_ord = 0; }
                const int32_t ord = t_vision_ord++;
                if (CyberpunkVR_VisionMap)
                    vision_note_surface(b.UAV.pResource, t_vrcam_node_active, rva, ord);
                // Only the second eye's is worth taking: MAIN composites its own. Copy it here,
                // while the transient is still alive -- the very next barrier batch recycles the
                // heap it sits in. Both extra conditions matter: without the size test the copy
                // alternated between the full and the half-res surface and leaked a texture per
                // alternation; without the ordinal it would take PS1040's output, not PS1213's.
                if (t_vrcam_node_active && rva == CyberpunkVR_VisionNode &&
                    ord == CyberpunkVR_VisionPick && vision_is_vrcam_full_size(vd)) {
                    hud_snapshot_copy(self, b.UAV.pResource, kSnapVision,
                                      D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
                    g_vision_tick.store(GetTickCount64(), std::memory_order_release);
                    InterlockedIncrement64(
                        reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugVisionSnaps));
                }
            }
        }
        if (b.Type == D3D12_RESOURCE_BARRIER_TYPE_UAV && b.UAV.pResource) {
            D3D12_RESOURCE_DESC ud{};
            if (mirror_get_resource_desc(b.UAV.pResource, &ud) &&
                ud.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER && ud.Width >= 4 &&
                ud.Width <= 32 &&
                (ud.Flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS)) {
                cull_count_note(self, b.UAV.pResource, static_cast<uint32_t>(ud.Width),
                                t_vrcam_node_active);
                cull_count_report();
            }
            continue;
        }
        if (b.Type != D3D12_RESOURCE_BARRIER_TYPE_TRANSITION ||
            b.Transition.StateBefore != D3D12_RESOURCE_STATE_UNORDERED_ACCESS ||
            b.Transition.StateAfter !=
                (D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE |
                 D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE) ||
            !b.Transition.pResource) {
            continue;
        }
        D3D12_RESOURCE_DESC rd{};
        if (!mirror_get_resource_desc(b.Transition.pResource, &rd)) continue;
        // Same transition, different resource: the per-tile light grid the lighting pass indexes.
        // Taken here because this is the barrier that publishes it, so its state is known and not
        // guessed. (The probe was written earlier but never called -- dead code, hence silence.)
        if (tile_is_grid(rd)) {
            tile_probe_copy(self, b.Transition.pResource, rd, b.Transition.StateAfter,
                            t_vrcam_node_active);
            continue;
        }
        if (rd.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER || rd.Width != 28) {
            continue;
        }
        // WHOSE BUFFER THIS IS, DECIDED BY THE VIRTUAL-CAMERA NAME HASH. ctx+0x28 reads 0 for
        // MAIN and g_vrcam_ctx_key for the port's VRCAM; every other RTT view has its own hash,
        // and so does the flat 960x540 desktop window -- which runs an exposure chain of its own
        // (third run in the capture, event 16942, against 61401 for VRCAM and 91137 for MAIN).
        //
        // The old test was `t_vrcam_node_active`, and its else-branch called EVERYTHING that is
        // not VRCAM "MAIN" -- the desktop window and every probe face included. So the mirror's
        // source was whichever of those transitioned its buffer last. That is why the copy could
        // be seen landing in the capture (our COPY_DEST barrier on Resource_124931) and still
        // leave the wrong numbers behind it.
        //
        // What those numbers are, measured at the lighting pass that reads them:
        //
        //     VRCAM  Resource_124931  f0 0.945388     f2 0.0003472  f3 1.05777
        //     MAIN   Resource_3894    f0 0.000328187  f2 1.0        f3 3047.04
        //
        // f0 is the pre-exposure every lighting shader multiplies its output by. The second
        // view's is 2880.6x MAIN's ON THE SAME SCENE, which is the white: it enters at Lighting
        // and every pass downstream inherits it. Its f2 is 1/2880.2 -- the view computed the
        // correction and never applied it, where MAIN's f2 is 1.
        if (!t_active_view_known) continue;      // unresolved view: never guess
        const uint64_t vrcam_key = g_vrcam_ctx_key.load(std::memory_order_acquire);
        const bool is_vrcam = (vrcam_key != 0 && t_active_view_key == vrcam_key);
        const bool is_main  = (t_active_view_key == 0);
        if (!is_vrcam && !is_main) continue;     // a probe face, or the desktop window
        if (is_vrcam) {
            ID3D12Resource* prev = g_expo_vrcam.exchange(
                b.Transition.pResource, std::memory_order_acq_rel);
            if (prev != b.Transition.pResource) {
                b.Transition.pResource->AddRef();
                if (prev) prev->Release();
            }
            CyberpunkVR_DebugExpoVrcam =
                reinterpret_cast<uint64_t>(b.Transition.pResource);
            expo_probe_copy(self, b.Transition.pResource, true);
            expo_mirror(self, b.Transition.pResource, true);
        } else {
            ID3D12Resource* prev = g_expo_main.exchange(
                b.Transition.pResource, std::memory_order_acq_rel);
            if (prev != b.Transition.pResource) {
                b.Transition.pResource->AddRef();
                if (prev) prev->Release();
            }
            CyberpunkVR_DebugExpoMain =
                reinterpret_cast<uint64_t>(b.Transition.pResource);
            expo_mirror(self, b.Transition.pResource, false);
            expo_probe_copy(self, b.Transition.pResource, false);
            expo_probe_report();
            tile_probe_report();
        }
    }
    // In-node, in-order tracking of the captured target's state (same recording
    // thread): gives the inline snapshot the EXACT StateBefore, no cross-frame guess.
    if (t_mirror_copy_node_active && t_mirror_copy_rtv) {
        for (UINT i = 0; i < count; ++i) {
            const D3D12_RESOURCE_BARRIER& b = barriers[i];
            if (b.Type == D3D12_RESOURCE_BARRIER_TYPE_TRANSITION &&
                b.Transition.pResource == t_mirror_copy_rtv) {
                t_mirror_src_state = (uint32_t)b.Transition.StateAfter;
                t_mirror_src_state_seen = true;    // observed, for this resource
            }
        }
    }
    // Track the tonemap RT0's real state so the epilogue snapshot uses the correct
    // StateBefore (else a hazard barrier stalls the engine's list = freeze).
    if (t_tm_rt0) {
        for (UINT i = 0; i < count; ++i) {
            const D3D12_RESOURCE_BARRIER& b = barriers[i];
            if (b.Type == D3D12_RESOURCE_BARRIER_TYPE_TRANSITION &&
                b.Transition.pResource == t_tm_rt0) {
                t_tm_rt0_state = (uint32_t)b.Transition.StateAfter;
                t_tm_rt0_state_seen = true;       // observed, for this resource
            }
        }
    }
    ID3D12Resource* output = g_captured_vrcam_res.load(std::memory_order_acquire);
    if (!output) return;
    for (UINT i = 0; i < count; ++i) {
        const D3D12_RESOURCE_BARRIER& b = barriers[i];
        if (b.Type != D3D12_RESOURCE_BARRIER_TYPE_TRANSITION ||
            b.Transition.pResource != output) continue;
        CyberpunkVR_DebugMirrorSrcState = (uint32_t)b.Transition.StateAfter;
        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
            &CyberpunkVR_DebugMirrorBarrierHits));
    }
}

}  // namespace detail
}  // namespace cvr
