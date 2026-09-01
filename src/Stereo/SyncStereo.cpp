// Path B (Sync Sequential)  M-B1 observational CALLER1 hook. See sync_stereo.h.

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
#pragma intrinsic(_ReturnAddress)
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

// Storage for the switch declared in logger.h. On by default: when stereo fails to come up,
// the install sequence is the first thing worth reading.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_StereoLog = 1;

// The barrel dot, published by the ImGui overlay (imgui_overlay.cpp) in NDC after its own zoom
// compensation. Taking the finished number rather than re-projecting keeps the two eyes' dots
// identical by construction.
extern "C" float    CyberpunkVR_BarrelDotNdcX;
extern "C" float    CyberpunkVR_BarrelDotNdcX2;   // the second eye's own value
extern "C" float    CyberpunkVR_BarrelDotNdcY;
extern "C" float    CyberpunkVR_BarrelDotRadiusPx;
extern "C" unsigned long long CyberpunkVR_BarrelDotTick;
extern "C" int      CyberpunkVR_BarrelDotSecondEye;
extern "C" unsigned long long CyberpunkVR_DebugBarrelDotDraws;


#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"
#include "Stereo/DetourRegistry.hpp"

namespace cvr {

extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugSecondaryManager = 0;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugSecondaryActive = 0;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugSecondaryBinding = 0;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugLastBatchManager = 0;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugLastWorkContext = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainNodeCalls = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugSecondaryNodeCalls = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMainNodeUnique = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugSecondaryNodeUnique = 0;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugMainNodeWorks[256] = {};
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugSecondaryNodeWorks[256] = {};
// WORK-verification: per-node (index parallel to *NodeWorks) accumulated rdtsc
// cycles + call counts inside the work_fn, per view. A node that DISPATCHES but
// internally SKIPS (early-out) shows near-zero avg cycles for that view -> lets
// us prove WORK (not just dispatch) and compare vrcam vs main per node.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainNodeCycles[256] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugSecondaryNodeCycles[256] = {};
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMainNodeCallN[256] = {};
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugSecondaryNodeCallN[256] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorCopyArms = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorGameCopies = 0;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugEyeItem = 0;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugEyeBuildMgr = 0;
extern "C" __declspec(dllexport) uint32_t  CyberpunkVR_DebugEyeBuildMode = 0xFFFFFFFF;
extern "C" __declspec(dllexport) uint64_t  CyberpunkVR_DebugEyeBuildHits = 0;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugEyeFa0 = 0;
extern "C" __declspec(dllexport) uint64_t  CyberpunkVR_DebugMainFgHits = 0;
extern "C" __declspec(dllexport) uint32_t  CyberpunkVR_DebugMainBuildMode = 0xFFFFFFFF;
// eye-X manager state (view/context count @+0x54, view array @+0x48, queued
// request slot @+296)  determines whether we can queue a context request.
extern "C" __declspec(dllexport) uint32_t  CyberpunkVR_DebugEyeMgrCount = 0xFFFFFFFF;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugEyeMgrArray = 0;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugEyeMgrQueued = 0;
extern "C" __declspec(dllexport) uint32_t  CyberpunkVR_DebugMainMgrCount = 0xFFFFFFFF;
// main view context-create identity (captured by the ctx-capture probe below).
extern "C" __declspec(dllexport) void*    CyberpunkVR_DebugMainManager = nullptr;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainCtxKey = 0;
extern "C" __declspec(dllexport) int      CyberpunkVR_DebugMainCtxA5 = -1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCtxCreateCalls = 0;

// Descriptor-heap probe/enlarge (Path A: full second eye needs a bigger
// shader-visible CBV_SRV_UAV heap than the engine's 1,000,000 request).
extern "C" __declspec(dllexport) uint64_t  CyberpunkVR_DebugDescHeapCreates = 0;
extern "C" __declspec(dllexport) uint32_t  CyberpunkVR_DebugDescHeapSVNum = 0;    // last shader-visible CBV_SRV_UAV NumDescriptors requested
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugDescHeapSVRetRva = 0; // caller RVA that requested it
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugDescHeapSVRetAbs = 0; // absolute caller return address
extern "C" __declspec(dllexport) uint32_t  CyberpunkVR_DebugDescHeapSVFlags = 0;
extern "C" __declspec(dllexport) uint64_t  CyberpunkVR_DebugDescHeapEnlarged = 0;

// ONE NAMED NAMESPACE WHERE THERE WAS AN ANONYMOUS ONE, and this is the change that unlocks the
// rest of the split.
//
// Everything from here to the close was in `namespace { }` -- 13,244 lines of INTERNAL linkage. That
// was correct while this was one translation unit and is the exact reason it could only ever be one:
// a detour, its RVA, its trampoline and the probe that drains it cannot be moved into separate files
// while none of them can be named from outside.
//
// `detail` rather than nothing: the names get external linkage so they can cross a file boundary,
// but they stay out of the global namespace, so nothing here can collide with the other twenty-odd
// translation units. A `using namespace detail;` after the close keeps every reference in the four
// public entry points below resolving exactly as it did.
//
// Behaviourally this is a no-op today. It is a prerequisite, committed on its own so that when the
// subsystems do move, the diff is the move and nothing else.
namespace detail {

// --- RE constants (image base 0x140000000, verified vs build 2.31) ----------
// The engine RVA table moved to include/Stereo/EngineRvas.hpp.
// Moved to src/Stereo/Mirror.cpp: the mirror's own D3D12 objects.

// ---- VRCAM identity: selected at runtime, never hardcoded -------------------
// The whole stereo path recognises the VRCAM view by ONE value: the CName hash of the RTT
// component's virtualCameraName, stored by the engine at view-ctx+0x28. There is now one
// authored entRenderToTextureCameraComponent per render resolution
// (vrcam_<W>x<H> / virtualCameraName vrcam_feed_<W>x<H>, all isEnabled=0), so that hash is
// DIFFERENT for every resolution and a literal would only ever match one of them. It is
// therefore derived from the selection file at init. Nothing here looks at aspect ratio or at
// resolution numbers to find the view -- only at the configured name.
// Selection file (written by the launcher, also read by modules/vrcam_select.lua which flips
// isEnabled on the matching component through the game's RTTI):
//   <game>\bin\x64\plugins\cyber_engine_tweaks\mods\CyberpunkVRPort_Stereo\vrcam.json
// with a fallback next to this DLL. CET sandboxes Lua file IO to the mod folder, which is why
// the canonical copy lives there and this side reaches out to it instead of the reverse.
// File access + CName hashing live in vrcam_config.h, shared with the launcher dialog.
// Defaults are the LEGACY single-component names, so deleting vrcam.json returns the mod to
// exactly the pre-per-resolution behaviour. That is the escape hatch if the expanded entity has
// not been imported yet: a stale selection would otherwise mean "no view recognised at all".
char     g_vrcam_component[96] = "vrcam";
char     g_vrcam_camera[96]    = "vrcam_feed";
// Atomic: the watcher thread below can replace it once CET reports the component's real
// virtualCameraName, while the render hooks compare against it every dispatch.
std::atomic<uint64_t> g_vrcam_ctx_key{0x8D23967F656EA945ULL};  // cname_hash("vrcam_feed")

// ---- MAIN gameplay view identity --------------------------------------------------------
// Aspect ratio cannot answer this: in VR MAIN renders square, exactly like VRCAM, so the old
// `key == 0 && aspect > 1.3f` test either misses MAIN or latches some other wide helper view.
// The engine does mark MAIN structurally -- Present and StartRender are MAIN-only nodes
// (vrcam calls/frame = 0.00 in docs/vrcam_node_audit_v2.md) -- but those nodes carry NO view
// ctx at work_context+0x18; they reach the view OBJECT through the work-context vtable
// (engine_re/dumps/nodes/work_000.md). Hence two steps, both cheap pointer compares after the
// first frame:
//   1. the node dispatcher records the view OBJECT when it sees a MAIN-only node;
//   2. the camera writer -- the one place that sees OBJECT and CTX together for every view --
//      pins the matching ctx.
// MAIN_PRESENT_WORK_RVA moved to Stereo/StereoInternal.hpp.
// MAIN_STARTRENDER_WORK_RVA moved to Stereo/StereoInternal.hpp.
std::atomic<uintptr_t> g_main_view_obj{0};
std::atomic<uintptr_t> g_main_view_ctx{0};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainObjBinds = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainCtxBinds = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainCtx = 0;
uintptr_t sl_view_obj(void* work_context);   // defined with the camera writer below

// True for MAIN's view ctx. The aspect test survives only as a bootstrap for the first frames,
// before the cache is warm; once it is, it is never consulted again -- which is what keeps
// this correct when MAIN goes square.
bool is_main_view(const void* view) {
    if (!view) return false;
    const auto* p = reinterpret_cast<const uint8_t*>(view);
    // Belt and braces: whatever the cache says, a view carrying the VRCAM camera name is never
    // MAIN. Without this, one bad pin silently disables every VRCAM branch that sits in an
    // `else if` after an is_main_view() test -- which is exactly how the fov write came out as
    // a no-op the first time round.
    if (*reinterpret_cast<const uint64_t*>(p + 0x28) != 0) return false;
    const uintptr_t cached = g_main_view_ctx.load(std::memory_order_acquire);
    if (cached) return reinterpret_cast<uintptr_t>(view) == cached;
    return *reinterpret_cast<const float*>(p + 0x98) > 1.3f;
}
// Render resolution of the selected component, parsed from its <W>x<H> name suffix. The
// mirror's RTV filter needs it to recognise the VRCAM target among every render target the
// game creates, and it must NOT be a literal вЂ” it changes with the pick.
std::atomic<uint32_t> g_vrcam_sel_w{0};
std::atomic<uint32_t> g_vrcam_sel_h{0};
// The resolution the launcher pick resolves to, parsed from the component name. The
// resolution override uses this as its fallback source, so MAIN still follows the pick
// even when vrport-launcher.ini was not written (dialog dismissed, file not writable).
extern "C" __declspec(dllexport) int CyberpunkVR_GetSelectedResolution(uint32_t* w, uint32_t* h) {
    const uint32_t sw = g_vrcam_sel_w.load(std::memory_order_relaxed);
    const uint32_t sh = g_vrcam_sel_h.load(std::memory_order_relaxed);
    if (!sw || !sh) return 0;
    if (w) *w = sw;
    if (h) *h = sh;
    return 1;
}
extern "C" __declspec(dllexport) const char* CyberpunkVR_VrcamComponentName() { return g_vrcam_component; }
// CName of the VRCAM camera COMPONENT (the "vrcam_<W>x<H>" object), not of its feed.
//
// This is how MAIN and VRCAM are told apart at the camera writer. The camera object is an
// Entity/IPlacedComponent (its vtable slot 68 returns that string) and carries its own
// component name at obj+0x40: measured live, the player's camera reads 0x6FCFDF926F11594E,
// which is exactly cname_hash("camera"). So the field is a per-instance identity that
// costs one load -- no view plumbing, no first/last/most-frequent guessing, and it keeps
// working across launches because it is a name hash, not an address.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_VrcamCamNameHash() {
    return cname_hash(g_vrcam_component);
}

// ...AND ITS BRAINDANCE TWIN. A braindance swaps the player for braindance_replacer.ent, which carries
// the same components under vrcam_braindance_<W>x<H> (see tools/gen_vrcam_assets.py); the selector
// enables whichever set belongs to the entity that is live, never both. The camera writer therefore has
// to accept either name as "this is the second view", or the twin renders from wherever its binding
// left it with nothing writing its transform -- measured as a white frame, or a view somewhere outside
// the city, in the left eye.
//
// Kept across the rollback of the render side to 0.1.5: this is a NAME, not a render mechanism, and
// the braindance camera writer in VrCore.cpp is its only caller.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_VrcamBdCamNameHash() {
    const char* sel = g_vrcam_component;                 // "vrcam_<W>x<H>"
    if (!sel || !*sel) return 0;
    static const char kPrefix[] = "vrcam_";
    const size_t plen = sizeof(kPrefix) - 1;
    if (std::strncmp(sel, kPrefix, plen) != 0) return 0;
    char name[96];
    const int n = std::snprintf(name, sizeof(name), "vrcam_braindance_%s", sel + plen);
    if (n <= 0 || static_cast<size_t>(n) >= sizeof(name)) return 0;
    return cname_hash(name);
}
extern "C" __declspec(dllexport) const char* CyberpunkVR_VrcamCameraName()    { return g_vrcam_camera; }
extern "C" __declspec(dllexport) uint64_t    CyberpunkVR_VrcamCtxKey()        { return g_vrcam_ctx_key.load(); }
// 1 when the key came from the file, 0 when the built-in default is in use.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugVrcamConfigLoaded = 0;

// Enable/disable the VRCAM component at runtime. The component's isEnabled lives behind the
// game's RTTI, which only Lua can reach, so this writes a request into the CET mod's bridge
// folder and modules/vrcam_select.lua acts on it. 1 = the selected component on (default),
// 0 = every vrcam component off, i.e. the engine stops rendering the second view entirely.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamEnabled = 1;
// Mirror/stereo chain diagnostics, always on (the comparisons they count already happen):
//   NodeHits    - node dispatches for a view matching our key   (0 => wrong key / not enabled)
//   CopyNodeHits- that view's RenderFinal2D node                (0 => VRCAM has no final blit)
//   RtvHits     - its render target captured                    (0 => OMSetRenderTargets missed)
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamNodeHits = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorCopyNodeHits = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorRtvHits = 0;
extern "C" __declspec(dllexport) void CyberpunkVR_SetVrcamEnabled(uint32_t on) {
    CyberpunkVR_VrcamEnabled = on ? 1u : 0u;
    if (vrcam_bridge_write("vrcam_enable.txt", on ? "1" : "0"))
        cvr::log("[vrcam] request %s -> bridge/vrcam_enable.txt", on ? "ENABLE" : "DISABLE");
    else
        cvr::log("[vrcam] FAILED to write bridge/vrcam_enable.txt (request %s ignored)",
                on ? "ENABLE" : "DISABLE");
}

// ---- ONE place that changes the VRCAM selection ------------------------------------------
//
// Four things describe this one choice and they have to move together: the component name,
// the camera name, the view key hashed from the camera name, and the resolution the RTV filter
// matches on. Updating a subset is not a small bug -- a key that names no live view means no
// second eye and no mirror window, while every log line still prints the component you
// expected. That is exactly how the last one hid.
bool g_vrcam_pick_authoritative = false;   // the launcher resolution decided it

// The VRCAM selection subsystem moved to src/Stereo/VrcamSelection.cpp.

// The CPU profiler moved to src/Stereo/Profiler.cpp.
thread_local bool t_mirror_copy_node_active = false;
thread_local ID3D12Resource* t_mirror_copy_rtv = nullptr;
thread_local DXGI_FORMAT t_mirror_copy_rtv_format = DXGI_FORMAT_UNKNOWN;
// Command list the vrcam RenderFinal2D node records into (stashed at RTV capture) and
// the engine's LAST transition StateAfter for the captured resource within the node
// (activation pattern is ALIASING + transition->RENDER_TARGET + Discard, so RT unless
// the node retransitions it later; tracked in hk_ResourceBarrier on the same thread).
thread_local ID3D12GraphicsCommandList* t_mirror_copy_list = nullptr;
thread_local uint32_t t_mirror_src_state =
    (uint32_t)D3D12_RESOURCE_STATE_RENDER_TARGET;
// AND WHETHER THAT NUMBER WAS EVER OBSERVED FOR THIS RESOURCE.
//
// The value above is a SEED, not a measurement: the engine is known to activate the target
// with ALIASING + transition->RENDER_TARGET + Discard, so RENDER_TARGET is the state it
// usually rests in. Usually is not always, and a barrier whose StateBefore is wrong is a lie
// told to the driver's resource-state machine.
//
// That machine is where this port's recurring crash lives. Read at instruction level on
// 2026-08-29: nvwgf2umx+0x757BC9 computes a resource state out of the D3D12 state masks
// (0x1098, 0x2088, 0x7E, & 0xA180FFFE), arrives at 0x200 = INDIRECT_ARGUMENT, and then does
// `mov rdx,[rsi+0x30]; cmp [rdx+0x88],2` on a binding whose resource record is null. Twelve
// crashes, byte-identical registers every time, and the D3D12 debug layer -- which validates
// exactly these transitions -- suppresses it completely.
//
// So: false until hk_ResourceBarrier has actually seen a transition of THIS resource on THIS
// thread. When it is false the copy is skipped rather than issued on an assumption.
thread_local bool t_mirror_src_state_seen = false;
// Work fn of the node currently recording on this thread (set at dispatch; nested
// dispatches clobber it -- acceptable for attribution probes).
thread_local uintptr_t t_current_node_work = 0;
// True while ANY vrcam-ctx node records on this thread (view attribution for hooks).
thread_local bool t_vrcam_node_active = false;

// View attribution for the ENGINE camera hooks (LocateCamera / PatchCamera /
// FinalCamera / NormalFov / Unifix / ProjStage in vr_core.cpp).
//
// Those hooks are mid-function byte patches with no notion of a view -- they were
// written when MAIN was the only one. IDA puts them inside per-view render-graph work:
// FinalCamera sits in sub_1407854C0, whose only caller is sub_140784ABC (PrepareScene
// node work); NormalFov is in the rect-compute node; Unifix in graph-request-build.
// All three run once PER VIEW, so enabling the VRCAM component makes each fire twice a
// frame -- once for a camera that is not MAIN -- and their shared per-frame state
// (g_lastLocateSeq, g_lastLocateQuat, g_renderedSeq) ends up describing whichever view
// ran last. MAIN is then submitted with a pose belonging to the other camera, which is
// the judder that appears only with the component enabled.
//
// The dispatcher already tags the executing view, so the hooks can simply ask.
extern "C" __declspec(dllexport) int CyberpunkVR_IsVrcamViewActive() {
    return t_vrcam_node_active ? 1 : 0;
}

// EXACT view identity, because "not VRCAM" is not the same as "is MAIN".
//
// The dispatcher already reads the view key at ctx+0x28. MAIN is key 0 -- that is how
// Detour_FlagCompute binds g_main_ctx -- and VRCAM is g_vrcam_ctx_key. Every OTHER view
// the engine runs (distant geometry, shadow and reflection views) carries its own key and
// sails straight through a vrcam-only gate.
//
// That matters for the camera hooks: forcing the head camera onto one of those views
// leaves its content composited against a camera it was not rendered with, so it stops
// being world-locked and slides with the head instead of staying put.
thread_local uint64_t t_active_view_key   = 0;
thread_local bool     t_active_view_known = false;

extern "C" __declspec(dllexport) int CyberpunkVR_IsMainViewActive() {
    return (t_active_view_known && t_active_view_key == 0) ? 1 : 0;
}
// Returns 0 when the caller is not inside a view-carrying node dispatch (the view is then
// genuinely unknown, not "MAIN"). Callers decide what to do with that.
extern "C" __declspec(dllexport) int CyberpunkVR_GetActiveViewKey(unsigned long long* out) {
    if (!t_active_view_known) return 0;
    if (out) *out = t_active_view_key;
    return 1;
}

// ---- WHICH VIEW OWNS A CAMERA OBJECT ------------------------------------------------
// LocateCamera runs outside the render-graph dispatch (measured: its vrcam gate never
// fired once), so the thread-local view tag above is useless there. But LocateCamera is
// also the ONLY stage where head orientation can be injected without artifacts -- it runs
// before culling, before the distant/imposter placement and before the weapon viewmodel
// is put into camera space, which is why writing later slides the world and drags the gun.
//
// So the view has to be attached to the CAMERA OBJECT instead. Both cameras are instances
// of the same class (vtable rva 0x2B031D0, measured live on both), and their addresses
// change every launch, so the object alone says nothing -- but the camera writer sees the
// view ctx (key: MAIN = 0, VRCAM = g_vrcam_ctx_key) and the view's own object at the same
// moment. Finding the camera through that ctx binds object -> view exactly, with no
// first/last/most-frequent guessing.
static std::atomic<uintptr_t> g_cam_obj_main{0};
static std::atomic<uintptr_t> g_cam_obj_vrcam{0};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCamObjMain    = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCamObjVrcam   = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCamBindMain   = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCamBindVrcam  = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCamScanFails  = 0;

// 0 = this object is not (yet) bound to any view, 1 = MAIN, 2 = VRCAM.
extern "C" __declspec(dllexport) int CyberpunkVR_ClassifyCamera(const void* obj) {
    const uintptr_t v = reinterpret_cast<uintptr_t>(obj);
    if (!v) return 0;
    if (v == g_cam_obj_main.load(std::memory_order_acquire))  return 1;
    if (v == g_cam_obj_vrcam.load(std::memory_order_acquire)) return 2;
    return 0;
}
// Dispatch nesting depth: frame-level totals are taken at depth 0 only, per-node rows at
// every depth (SceneDrv re-enters the dispatcher once per scene pass).
thread_local int t_prof_disp_depth = 0;
// Inclusive time of the child nodes dispatched by the node currently on this thread's
// stack; lets each node report self = inclusive - children. Only touched when profiling.
thread_local int64_t t_prof_child_ticks = 0;
// Tonemap OUTPUT capture (crash-safe fix): the tonemap 2-MRT pass writes RT0 = the
// tonemapped (dark/correct) post color. We snapshot RT0 into our committed g_stable_tex
// in the valid window (tonemap node epilogue) and the mirror/eye reads OUR buffer,
// bypassing Final2D's flapping version selection entirely. Pure copy of an engine-valid
// RT => no index/resolver/generation manipulation => cannot crash (unlike all the
// resolver-layer attempts). Set in the OM hook when the identified tonemap 2-MRT binds.
thread_local ID3D12Resource* t_tm_rt0 = nullptr;
thread_local ID3D12GraphicsCommandList* t_tm_rt0_list = nullptr;
// consumed flag: set false when the OM hook captures RT0; the FIRST node-dispatch
// epilogue after that consumes it once (= the tonemap node's own epilogue, its list
// still open). Avoids fragile per-dispatch reset / t_current_node_work checks that
// nested sub-node dispatches broke (DebugTonemapSnaps stayed 0).
thread_local bool t_tm_consumed = true;
// RT0's real state, tracked in hk_ResourceBarrier during the tonemap node (RT0 is NOT
// necessarily RENDER_TARGET at the node epilogue -> the wrong barrier StateBefore made
// the engine's Close/submit stall = the freeze). Init to RENDER_TARGET at capture.
thread_local uint32_t t_tm_rt0_state = (uint32_t)D3D12_RESOURCE_STATE_RENDER_TARGET;
// Same rule as t_mirror_src_state_seen above: the initialiser is a seed, not an observation.
thread_local bool t_tm_rt0_state_seen = false;
// The finished-frame slot, per thread for the reason recorded beside CyberpunkVR_FinalGrab: the
// engine records nodes on several threads at once, so a global handed the epilogue a command list
// belonging to ANOTHER thread, or one from the previous frame, by then closed and recycled.
//
// t_final_writes is carried for fidelity with the mechanism as authored and stays 0 in this branch:
// the two sites that incremented it hang off vrcam_list_note, which is 0.1.6-only, and the copy gate
// never consulted it there either.
thread_local bool t_tm_from_final = false;
thread_local ID3D12Resource*            t_final_res = nullptr;
thread_local uint32_t                   t_final_state = 0;
thread_local ID3D12GraphicsCommandList* t_final_list = nullptr;
thread_local uint32_t                   t_final_writes = 0;
// True once the tonemap RT0 source is available (DLSS path). When set, the Final2D
// copy is skipped (tonemap source wins); when NOT set (no DLSS), the Final2D copy runs
// as fallback -> fixes the "washed out without DLSS" regression.
std::atomic<bool> g_have_tonemap_source{false};
extern "C" __declspec(dllexport) int32_t CyberpunkVR_StableFromTonemap = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugTonemapSnaps = 0;
// Adapted-exposure accumulators (28-byte UAV buffers, one per view; identified in the
// EXPOSURE capture: vrcam=23828, main=5712). Captured live by barrier signature:
// BUFFER W=28 transitioning UNORDERED_ACCESS -> PS|NPS right after the 1x1x1
// adaptation dispatch, attributed by t_vrcam_node_active.
std::atomic<ID3D12Resource*> g_expo_vrcam{nullptr};
std::atomic<ID3D12Resource*> g_expo_main{nullptr};
// The engine's per-frame constants (b0 in the composite): 30 float4, first float is the time the
// HUD flicker runs on. Captured in hk_CopyBufferRegion by its unique 480-byte upload.
std::atomic<ID3D12Resource*> g_frame_cb{nullptr};
// The composite's own constants (b6): curvature, glow weights, aberration, halo.
std::atomic<ID3D12Resource*> g_hud_cb{nullptr};
// True once g_hud_cb points at OUR OWN copy taken from the ring. From then on the copy-path
// capture must keep its hands off: it releases whatever it displaces, and displacing our buffer
// would free it while g_hud_cb_copy_ptr still points into it.
std::atomic<bool> g_hud_cb_from_ring{false};

// Persistent CPU view of an engine UPLOAD heap, so a constant upload can be identified by its
// contents as it goes past. Mapping an upload resource is refcounted and read-only here; the
// mapping is deliberately never released, since the heaps live for the session.
// MappedUpload moved to Stereo/StereoInternal.hpp.
// 64 SLOTS, AND REFUSALS DO NOT LIVE HERE. There used to be eight, shared between mapped rings
// and the memo of resources that turned out not to be upload heaps -- and since every
// CopyBufferRegion source is probed, the eight filled within seconds, mostly with refusals.
// After that upload_map_read returned nullptr for everything forever, so the composite's
// constants could never be found in the ring at all: a whole 17k-line session logged not one
// "found in the upload ring", and the only capture came from the weaker content test below,
// thousands of frames in. The refusal memo is a pure optimisation, so it gets its own ring that
// may be overwritten; a re-probe costs one GetHeapProperties.
std::array<MappedUpload, 64> g_upload_maps{};
uint32_t g_upload_map_n = 0;
static std::array<ID3D12Resource*, 64> g_upload_refused{};
static uint32_t g_upload_refused_w = 0;
static bool g_upload_map_full_logged = false;
std::mutex g_upload_map_mtx;

// ---- finding the composite's constants where they actually live -----------------------------
//
// They are never copied anywhere: the capture shows the CBV pointing straight into the upload
// ring (Resource_41 @ 108361216), so the engine writes them there and binds them in place.
// Watching CopyBufferRegion for them was therefore looking in the wrong place entirely.
//
// The ring is CPU-visible and already mapped for us, so instead we look for them by fingerprint.
// A 256-byte-aligned block qualifies only if register 16 zw is exactly the HUD surface size (the
// composite's target) AND the rest is in range for what it claims to be -- curvature small, glow
// weights and saturation in [0,1], aberration tiny. Nothing else in the engine's constant traffic
// satisfies all of that at once.
//
// The values are graphics settings, so they change only when the user changes one: found once and
// re-checked every few seconds, at a cost of a few milliseconds on the present thread.
ID3D12Resource* g_hud_cb_copy = nullptr;   // our own 512-byte upload CB
uint8_t* g_hud_cb_copy_ptr = nullptr;
uint64_t g_hud_cb_scan_tick = 0;

bool hud_cb_block_plausible(const uint8_t* p, float w, float h) {
    __try {
        const float* r = reinterpret_cast<const float*>(p);
        if (r[16 * 4 + 2] != w || r[16 * 4 + 3] != h) return false;
        const float cxk = r[3 * 4 + 0], cyk = r[3 * 4 + 1];
        if (!(fabsf(cxk) < 0.5f && fabsf(cyk) < 0.5f)) return false;
        const float w1 = r[8 * 4 + 2], w2 = r[8 * 4 + 3], w4 = r[9 * 4 + 0];
        if (!(w1 >= 0.0f && w1 <= 1.0f && w2 >= 0.0f && w2 <= 1.0f &&
              w4 >= 0.0f && w4 <= 1.0f)) return false;
        const float ab = r[6 * 4 + 3], sat = r[6 * 4 + 2];
        if (!(ab >= 0.0f && ab < 0.01f && sat >= 0.0f && sat <= 4.0f)) return false;
        const float bg = r[5 * 4 + 0], bl = r[5 * 4 + 1];
        if (!(bg >= 0.0f && bg <= 4.0f && bl >= 0.0f && bl <= 8.0f)) return false;
        // A block of zeros passes every range test above -- and the ring is full of them. Demand
        // that the settings actually look set: a halo, a glow and a curvature all present.
        return bg > 1e-4f && (w1 > 1e-4f || w2 > 1e-4f || w4 > 1e-4f) &&
               (fabsf(cxk) > 1e-6f || fabsf(cyk) > 1e-6f);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

// The same test, applied to an upload as it is copied rather than to a block sitting in the ring.
//
// It used to check ONLY that register 16 zw matched the target size, on the grounds that nothing
// else carries that pair -- and the log says otherwise: it captured blocks with curvature
// (0, nan), (-1.0e35, nan) and (-4.9e35, nan). A size pair is a weak fingerprint on its own
// because it is two floats that any coincidence can produce; the range checks are what make it
// unambiguous. Two capture paths for one buffer had two different ideas of "valid", and the
// permissive one always won, because it ran on every copy while the strict one only stored when
// nothing had been found yet.
bool hud_cb_content_matches(const uint8_t* base, UINT64 off, float w, float h,
                                   float* curvature_out) {
    if (!base) return false;
    if (!hud_cb_block_plausible(base + off, w, h)) return false;
    __try {
        const float* r3 = reinterpret_cast<const float*>(base + off + 3 * 16);
        curvature_out[0] = r3[0];
        curvature_out[1] = r3[1];
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

const uint8_t* upload_map_read(ID3D12Resource* res) {
    if (!res) return nullptr;
    std::lock_guard<std::mutex> lk(g_upload_map_mtx);
    for (uint32_t i = 0; i < g_upload_map_n; ++i)
        if (g_upload_maps[i].res == res) return g_upload_maps[i].ptr;
    for (ID3D12Resource* r : g_upload_refused)
        if (r == res) return nullptr;
    if (g_upload_map_n >= g_upload_maps.size()) {
        if (!g_upload_map_full_logged) {
            g_upload_map_full_logged = true;
            log("[hud] upload-map table full at %u rings -- no further heaps will be mapped",
                g_upload_map_n);
        }
        return nullptr;
    }
    auto refuse = [&]() {
        g_upload_refused[g_upload_refused_w] = res;
        g_upload_refused_w = (g_upload_refused_w + 1) % g_upload_refused.size();
    };
    D3D12_HEAP_PROPERTIES hp{};
    D3D12_HEAP_FLAGS hf{};
    if (FAILED(res->GetHeapProperties(&hp, &hf)) || hp.Type != D3D12_HEAP_TYPE_UPLOAD) {
        refuse();
        return nullptr;
    }
    void* ptr = nullptr;
    D3D12_RANGE none{0, 0};
    if (FAILED(res->Map(0, &none, &ptr)) || !ptr) {
        refuse();
        return nullptr;
    }
    res->AddRef();
    const D3D12_RESOURCE_DESC ud = res->GetDesc();
    g_upload_maps[g_upload_map_n++] = { res, static_cast<uint8_t*>(ptr), ud.Width,
                                       res->GetGPUVirtualAddress() };
    return static_cast<const uint8_t*>(ptr);
}
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugExpoVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugExpoMain = 0;
// Tonemap-INPUT probe: exposure is proven stable, so the flap is in one of the
// tonemap's texture inputs (constant transform x two static inputs = two bit-exact
// output states). Capture every >=1000px texture transitioned to PS-readable while
// the vrcam tonemap node records; sample an 8x8 center block of each per frame and
// split the means by the final image's bright/normal state -> the culprit input has
// a large split, innocent inputs ~0.
// struct TmInCap now lives in Stereo/StereoInternal.hpp: the mirror's luma probe indexes the array
// below and reads its members, so the type has to be complete on both sides.
// ONE global capture set covering the whole vrcam frame: every large texture
// transitioned to a read state during ANY vrcam node, tagged with the capturing
// node's work RVA. Entries persist across frames (session resources are stable;
// dedupe keeps the set converged) -- stale entries sample stable content = innocent.
TmInCap                  g_tm_in[24];
std::atomic<uint32_t>    g_tm_in_n{0};
std::atomic<uint32_t>    g_tm_in_rva[24] = {};    // capturing node work RVA
ID3D12Resource*          g_ti_rb[4] = {};         // 64KB readback per slot
uint8_t*                 g_ti_map[4] = {};
uint32_t                 g_ti_count[4] = {};
ID3D12Resource*          g_ti_src[4][24] = {};
uint32_t                 g_ti_fmt[4][24] = {};
uint32_t                 g_ti_tag[4][24] = {};    // node work RVA
bool                     g_ti_valid[4] = {};
// Dedupe-append (POD-only, no locks: worst case a rare duplicate, harmless).
// Keeps the FIRST capturing node's RVA (where the texture first became readable).
void tm_set_push(ID3D12Resource* res, uint32_t state, uint32_t fmt,
        uint32_t rva) {
    if (!res) return;
    const uint32_t n = g_tm_in_n.load(std::memory_order_acquire);
    for (uint32_t k = 0; k < n && k < 24; ++k) {
        if (g_tm_in[k].res.load(std::memory_order_relaxed) == res) {
            g_tm_in[k].state.store(state, std::memory_order_relaxed);
            return;
        }
    }
    if (n < 24) {
        res->AddRef();      // entries outlive the frame; engine may free transients
                            // on graph rebuilds (VrcamDlss toggle) -> dangling ptr ->
                            // AV in GetDesc (crash 20260720-165958, read @0x3C).
        g_tm_in[n].res.store(res, std::memory_order_relaxed);
        g_tm_in[n].state.store(state, std::memory_order_relaxed);
        g_tm_in[n].fmt.store(fmt, std::memory_order_relaxed);
        g_tm_in_rva[n].store(rva, std::memory_order_relaxed);
        g_tm_in_n.store(n + 1, std::memory_order_release);
    }
}
// Tonemap node (work RVA 0x768510) resolves its frame-graph resources with IDs
// ((work_context+0x34)<<24) XOR const -- +0x34 is the per-view graph-instance salt.
// If vrcam's tonemap salt flaps between its own and main's value, it resolves MAIN's
// adapted-exposure buffer on those frames => the two bit-exact luma states. Log salt
// TRANSITIONS for both views + export last values for x64dbg polling.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugTmSaltVrcam = 0xFFFFFFFF;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugTmSaltMain = 0xFFFFFFFF;
static std::atomic<uint32_t> g_tmsalt_last_vrcam{0xFFFFFFFF};
static std::atomic<uint32_t> g_tmsalt_last_main{0xFFFFFFFF};
// THE FLAP SELECTOR (decompiled from the RenderFinal2D node, sub_140209FF8):
//   salt = (ctx[0x30] & 1) ? 0 : ctx[0x34];   // bit0 => "borrow MAIN's instance"
//   source_id = (salt<<24) ^ 0x3D7E6258;
// With forced VrcamDlss the graph's "does this view own a post buffer" decision races
// -> bit0 flaps -> the final blit alternately reads MAIN's post buffer (bright) and
// vrcam's own (normal): two stable sources = two bit-exact luma states. The vrcam
// post chain provably runs every frame (histogram 6/frame, tonemap each frame), so
// borrowing is always wrong here: force bit0=0 for vrcam-ctx nodes.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_ForceOwnPost = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugOwnPostFixes = 0;
static std::atomic<uint32_t> g_ctx30_last_vrcam{0xFFFFFFFF};
static std::atomic<uint32_t> g_ctx30_last_main{0xFFFFFFFF};
// Resolver probe: sub_1401F3D20 = frame-graph "declared ID -> physical index" query
// used by every node right before writing descriptors. Gated to the vrcam
// RenderFinal2D / tonemap recording threads it shows, PER FRAME, which salted ID the
// pass asked for and which physical index came back -> the selection flap becomes
// directly visible at the consumer (no more stage guessing).
using Resolve3D20Fn = __int64(__fastcall*)(__int64, uint32_t*, uint32_t*, __int64);
static Resolve3D20Fn g_orig_resolve3d20 = nullptr;
extern uint8_t* g_exe_base;     // defined below with the other engine globals
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugResolveHits = 0;
// ROOT CAUSE / ARCHITECTURAL FIX (layer 2 = deterministic version binding).
// The frame graph versions each logical resource; a consumer resolves "the current
// generation" of a logical id via the per-view declare counter v7 = *(a1+24064*salt+64)
// at the moment ITS declare runs. For MAIN the build order is fixed, so Final2D always
// declares AFTER tonemap's write => resolves the post-tonemap (final) generation. For
// the FORCED-DLSS vrcam view the parallel graph build is not deterministically ordered,
// so vrcam's Final2D declare lands before/after tonemap's write across frames => it
// resolves an EARLIER (pre/less-processed, brighter) generation on some frames and the
// final generation on others: the bright/normal flap.
// Fix: bind the consumer to the PRODUCER'S LATEST generation. In the resolver, track
// per (id, physical index) the generation it was resolved at; for vrcam's Final2D
// resolve of a post-color id, override the returned index to the recently-seen version
// with the MAXIMUM generation (= what the last writer produced). Recency window guards
// epoch changes. Post-color id = (salt<<24)^0x3D7E6258; salt only alters the top byte,
// so low 3 bytes 0x7E6258 match post-color for ANY view -> generalizes to both eyes.
constexpr uint32_t POSTCOLOR_LOW = 0x007E6258u;
// SAFETY DEFAULT 0: physical indices are PER-FRAME transient (proven: [pv] shows fin
// idx changing every frame at a FIXED gen=2), so substituting any remembered index =
// freed slot next frame = null deref (crashes 180201/180846). Override stays off until
// we identify a PERSISTENT correct index via the safe luma<->finidx correlation below.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_FixPostVersion = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPinApplied = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugFinNatural = 0;   // last natural fin idx
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugFinPinned  = 0;   // last pinned fin idx
// Tonemap (producer) OUTPUT: the index it resolves at the MAX generation this frame
// (= post-tonemap write, the dark/correct version). Captured read-only; consumed by
// Final2D by INDEX substitution -- which is crash-safe (match-tm ran stably for minutes,
// only bright because it used tonemap's LAST resolve = an input read, not the output).
static std::atomic<uint32_t> g_tm_post_idx{0};
static std::atomic<uint32_t> g_tm_post_gen{0};
static std::atomic<uint32_t> g_tm_post_seq{0};
static std::atomic<uint32_t> g_pv_seq{1};
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugTmPostIdx = 0;
// STAGE PROBE (path-A, 17): the vrcam post-color id 0x3C7E6258 is resolved at MANY
// pipeline STAGES (the resolver's a4/r9 salt = a stage index 36..71, live-confirmed).
// This maps every stage's resolved physical index per frame and dumps the whole map at
// the fin (mirror-blit) resolve, so we can diff DLSS on/off and see which stage's physical
// the fin consumer should read. Read-only; gated by CyberpunkVR_StageProbe (default 0).
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_StageProbe = 1;
static uint32_t g_stage_idx[128] = {0};   // salt(stage) -> last resolved physical index (vrcam 0x3C)
static uint32_t g_stage_idx_main[128] = {0}; // salt(stage) -> physical (MAIN 0x3D) for aliasing test
static uint32_t g_stage_v7 [128] = {0};   // salt(stage) -> gen counter at resolve
static std::atomic<uint32_t> g_stage_frame{0};
// WRITE-ORDER PROBE (root-cause): capture, per frame, the ORDER of vrcam post-color
// type-4 (WRITE) declares at salt70 with their generation + caller, then dump at the fin
// resolve tagged with fin's resolved index (bright/dark). Reveals whether the DLSS-raw
// write vs the composition/tonemap write lands LAST (highest gen) before Final2D reads,
// and whether that order races across bright/dark frames = the true root. Reliable C++
// gens (no x64dbg hex bug). Gated by CyberpunkVR_StageProbe. Declare hook on sub_1401F0F80.
using DeclareFn = char(__fastcall*)(__int64, __int64, unsigned __int8, __int64);
static DeclareFn g_orig_declare = nullptr;
struct S70Write { uint32_t gen; uint32_t caller_rva; uint8_t type; };
static S70Write g_s70w[24] = {};
static volatile long g_s70w_n = 0;
struct S70Res { uint32_t caller_rva; uint32_t phys; };
static S70Res g_s70res[20] = {};
static volatile long g_s70res_n = 0;


// SAME-FRAME DE-ALIAS (path-A root fix, doc 23). Proven via [s70res]: within ONE frame the
// vrcam post-color is resolved by node377 at RET-RVA 0x378178 -> a STABLE correct physical
// (31691, vrcam's own), and by the DLSS temporal pass sub_140378224 at RET-RVA 0x3783CF ->
// the DISPLAYED physical that FLAPS (dark=31691 vrcam / bright=main-range, aliased from the
// shared transient pool). Both resolves are the SAME id in the SAME frame => both physicals
// are LIVE this frame => forcing the flapping (displayed) resolve to the stable one is
// CRASH-SAFE (unlike the cross-frame pin that used freed indices). Refreshed every frame
// (0x378178 is stable per scene), recency-guarded so a missed cache just falls back to
// natural (flap, no crash) rather than using a stale index.
constexpr uint32_t POST_STABLE_RET_RVA = 0x378178u;  // node377 stable post-color resolve
constexpr uint32_t POST_FLAP_RET_RVA   = 0x3783CFu;  // sub_140378224 resolve = displayed
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_DealiasPostColor = 1;   // default ON
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugDealiasHits = 0;
static std::atomic<uint32_t> g_post_stable_idx{0};
static std::atomic<uint32_t> g_post_stable_seq{0};
static std::atomic<uint32_t> g_dealias_seq{1};
static __int64 __fastcall Detour_Resolve3D20(__int64 reg, uint32_t* out,
        uint32_t* idp, __int64 r9) {
    const void* ra = _ReturnAddress();
    const __int64 r = g_orig_resolve3d20(reg, out, idp, r9);
    if (!out || !idp) return r;
    const uint32_t id = idp[0];
    if ((id & 0x00FFFFFFu) != POSTCOLOR_LOW) return r;      // post-color ids only
    // --- SAME-FRAME DE-ALIAS of vrcam post-color: cache node377's STABLE resolve (0x378178);
    // the DISPLAYED resolve is the mirror-blit consumer (in_fin, salt71) which flaps -> we
    // override THAT below to the cached stable physical (same-frame => live => crash-safe).
    if (id == 0x3C7E6258u) {
        const uint32_t rva = (uint32_t)(reinterpret_cast<uintptr_t>(ra)
            - reinterpret_cast<uintptr_t>(g_exe_base));
        const uint32_t s = g_dealias_seq.fetch_add(1, std::memory_order_relaxed);
        if (rva == POST_STABLE_RET_RVA) {
            g_post_stable_idx.store(*out, std::memory_order_release);
            g_post_stable_seq.store(s, std::memory_order_release);
        }
    }
    const bool in_fin = t_mirror_copy_node_active;          // vrcam Final2D consumer
    const bool in_tm  = t_vrcam_node_active && t_current_node_work ==
        reinterpret_cast<uintptr_t>(g_exe_base) + TONEMAP_WORK_RVA;
    // --- MAIN(0x3D) post-color: record per-salt physical to test main<->vrcam aliasing ---
    if (CyberpunkVR_StageProbe && id == 0x3D7E6258u) {
        const uint8_t st = (uint8_t)r9;
        if (st < 128) g_stage_idx_main[st] = *out;
    }
    // --- STAGE PROBE: record EVERY vrcam(0x3C) post-color resolve, all stages ---
    if (CyberpunkVR_StageProbe && id == 0x3C7E6258u) {
        const uint8_t st = (uint8_t)r9;
        uint32_t sv7 = 0;
        if (reg) { __try { sv7 = *reinterpret_cast<uint32_t*>(
            reg + 24064ull * st + 64); } __except (EXCEPTION_EXECUTE_HANDLER) { sv7 = 0; } }
        g_stage_idx[st] = *out;
        g_stage_v7[st]  = sv7;
        // salt70: record (caller-RVA -> resolved physical) this frame to compare the
        // WRITE resolve (node377 / sub_140378224) vs the READ resolve (Final2D 0x209xxx).
        if (st == 70) {
            const uintptr_t ra = reinterpret_cast<uintptr_t>(_ReturnAddress());
            const uint32_t rva = (uint32_t)(ra - reinterpret_cast<uintptr_t>(g_exe_base));
            long i = _InterlockedIncrement(&g_s70res_n) - 1;
            if (i >= 0 && i < 20) { g_s70res[i].caller_rva = rva; g_s70res[i].phys = *out; }
        }
        if (in_fin) {
            // fin (mirror blit) runs late in the frame. Dump the salt70 WRITE-ORDER
            // (accumulated by Detour_Declare this frame) tagged with fin's resolved index,
            // so bright vs dark frames can be compared: which writer (caller) held the
            // LAST/highest gen before Final2D read = what determines the flap.
            const uint32_t fr = g_stage_frame.fetch_add(1, std::memory_order_relaxed);
            const long wn = g_s70w_n;
            if (fr < 600u) {                                 // cap total lines
                char buf[700]; int n = 0;
                n += _snprintf_s(buf + n, sizeof(buf) - n, _TRUNCATE,
                    "[s70res] fr=%u finIdx=%u :", fr, *out);
                const long rn = g_s70res_n;
                for (long i = 0; i < rn && i < 20 && n < (int)sizeof(buf) - 32; ++i)
                    n += _snprintf_s(buf + n, sizeof(buf) - n, _TRUNCATE,
                        " %X=%u", g_s70res[i].caller_rva, g_s70res[i].phys);
                log("%s", buf);
            }
            g_s70w_n = 0;                                    // reset for next frame
            g_s70res_n = 0;
        }
    }
    if (!in_fin && !in_tm) return r;                        // vrcam post nodes only
    InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
        &CyberpunkVR_DebugResolveHits));
    uint32_t v7 = 0;
    if (reg) {
        __try { v7 = *reinterpret_cast<uint32_t*>(reg + 24064ull * (uint8_t)r9 + 64); }
        __except (EXCEPTION_EXECUTE_HANDLER) { v7 = 0; }
    }
    const uint32_t seq = g_pv_seq.fetch_add(1, std::memory_order_relaxed);
    if (in_tm) {
        // Track tonemap's OUTPUT = its resolve at the MAX generation. gen is stable per
        // frame (~29) and recurs, so ">=" keeps latching the output; a thread-local
        // gen-drop check resets across frames.
        thread_local uint32_t tl_last_tm_gen = 0;
        uint32_t pg = g_tm_post_gen.load(std::memory_order_acquire);
        if (v7 + 4u < tl_last_tm_gen) pg = 0;              // gen dropped => new frame
        if (v7 >= pg) {
            g_tm_post_idx.store(*out, std::memory_order_release);
            g_tm_post_gen.store(v7, std::memory_order_release);
            g_tm_post_seq.store(seq, std::memory_order_release);
            CyberpunkVR_DebugTmPostIdx = *out;
        }
        tl_last_tm_gen = v7;
    } else {  // in_fin consumer (the mirror-blit / displayed resolve)
        CyberpunkVR_DebugFinNatural = *out;
        // SAME-FRAME DE-ALIAS: the displayed resolve flaps (dark=vrcam-own / bright=main
        // aliased). Force it to node377's stable resolve (0x378178) captured THIS frame
        // (same frame => live => crash-safe). Recency guard => fallback to natural on miss.
        if (CyberpunkVR_DealiasPostColor && id == 0x3C7E6258u) {
            const uint32_t si = g_post_stable_idx.load(std::memory_order_acquire);
            const uint32_t ss = g_post_stable_seq.load(std::memory_order_acquire);
            const uint32_t ds = g_dealias_seq.load(std::memory_order_relaxed);
            // Wide recency window: node377's 0x378178 resolve runs EVERY frame before the
            // mirror blit (proven: s70res missing-378178=0), so `si` is always same-frame
            // fresh -> a large window is safe (staleness only if 378178 stops for many
            // frames, e.g. DLSS off/menu, after which override self-disables). The async
            // per-frame variation in the resolve-count gap (was occasionally >48 -> rare
            // bright flash on static scenes) is covered here.
            if (si && si != *out && (ds - ss) < 512u) {
                *out = si;
                InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
                    &CyberpunkVR_DebugDealiasHits));
            }
        }
        if (CyberpunkVR_FixPostVersion) {
            const uint32_t tmidx = g_tm_post_idx.load(std::memory_order_acquire);
            const uint32_t tmseq = g_tm_post_seq.load(std::memory_order_acquire);
            // Fresh (tonemap resolved this/last frame) => its index is current-frame
            // valid => crash-safe substitution (proven: match-tm never crashed).
            if (tmidx && (seq - tmseq) < 8u && tmidx != *out) {
                *out = tmidx;
                InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
                    &CyberpunkVR_DebugPinApplied));
            }
        }
        CyberpunkVR_DebugFinPinned = *out;
    }
    { static std::atomic<uint32_t> lg{0};
      if (lg.fetch_add(1) < 60)
          log("[pv] %s id=%08X idx=%u v7=%u", in_fin ? "fin" : "tm ", id, *out, v7); }
    return r;
}
// Stable committed snapshot of the vrcam final. Filled INLINE inside the engine's own
// command list at the END of the vrcam RenderFinal2D node: after the composite draw is
// recorded and BEFORE any later pass records the ALIASING barrier that hands the
// transient's heap memory to another resource. Proven root of the bright/dark flicker:
// the deferred copy ran ~30 events AFTER that hand-off (ev95217 vs ev95245 in the
// 3-frame capture) and raced main's reuse of the shared heap (Heap_96361 @0) => read
// main's content on some frames. Committed => never aliased; rests in COMMON between
// frames; only the game queue touches it => no cross-queue hazard. The deferred mirror
// hop reads THIS instead of the transient. Later this is the OpenXR eye-submit surface.
ID3D12Resource*     g_stable_tex = nullptr;
D3D12_RESOURCE_DESC g_stable_desc{};
std::mutex          g_stable_mtx;
std::atomic<bool>   g_stable_fresh{false};
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_StableCopy = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugStableCopies = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugStableSkips = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugStableSrcState = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_MirrorOutput;  // defined below

// ---- who wants the VRCAM colour snapshot -----------------------------------------------
// The snapshot used to exist purely for the desktop mirror, so its capture was gated on
// MirrorOutput. The OpenXR submit now reads the SAME resource for the right eye, and it must
// be able to do so with the mirror window closed -- that window is a capture convenience,
// not a prerequisite for stereo.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_StereoEyeCapture = 1;
bool stereo_eye_capture_wanted() {
    return CyberpunkVR_MirrorOutput != 0 || CyberpunkVR_StereoEyeCapture != 0;
}
// When the last snapshot was taken. The submit needs LIVENESS, not existence: g_stable_fresh
// latches true on the first copy and never clears, so on its own it would keep handing the
// right eye a frozen image after the second view stops (menu, component off) -- one eye
// live and one eye stuck is far worse than plain mono.
std::atomic<uint64_t> g_stable_tick{0};
// The same instant on a QPC clock, in MICROSECONDS. g_stable_tick above is milliseconds from
// GetTickCount64 and three readers compare it that way; this one exists only so the reported age can
// resolve a frame, because 15.6 ms of granularity cannot.
std::atomic<uint64_t> g_stable_tick_us{0};
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_StereoEyeMaxAgeMs = 250;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugVrcamEyeAgeMs = 0xFFFFFFFFu;
// THE FOUR PRECONDITIONS OF THE SECOND EYE, COUNTED SEPARATELY.
//
// g_stable_tick above is the clock the submit reads: older than StereoEyeMaxAgeMs and
// GetVrcamEyeTextureFresh hands back null, which is mono. Everything that can stop that clock
// happens in one node epilogue, and until now every one of those exits was silent -- which is how
// the last watchdog managed to stay quiet while the eye died. It sits inside mirror_publish_output,
// so it could only ever report on frames where that function RAN; the snapshot needs two things
// publish does not (the engine's command list, and a hook entry for it), and a failure there is
// invisible from in there.
//
// So each step gets its own counter and the report is printed where the decision is actually made.
std::atomic<uint64_t> g_eye_node_hits{0};   // the vrcam CopyToTexture node ran
std::atomic<uint64_t> g_eye_no_rtv{0};      // ...but no output RTV was latched in it
std::atomic<uint64_t> g_eye_no_list{0};     // ...RTV yes, but no command list to record on
std::atomic<uint64_t> g_eye_copy_calls{0};  // the snapshot copy was actually attempted
// Luma oscilloscope: turn the bright/normal alternation into NUMBERS. Each vrcam frame
// the deferred hop also copies an 8x8 center block of the stable snapshot into a
// readback slot (ringed with the existing copy fence); when a slot is reused its
// completed block is decoded (R11G11B10F) and average luma is accumulated (a) by frame
// parity and (b) as mean |dL| between CONSECUTIVE frames (parity-proof oscillation
// amplitude). One log line per ~120 vrcam frames -> fixes get A/B'd by numbers.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_LumaProbe = 1;   // diagnostics OFF (flicker RE done) -> no readbacks / [luma][cbflap][chain]... log spam
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugLumaEvenMilli = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugLumaOddMilli = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugLumaDeltaMilli = 0;
// Waveform: log the first N per-frame luma samples ("[lumaw] fr=.. L=..") so the
// oscillation SHAPE is visible (strict period-2 square = history ping-pong misread;
// multi-frame sawtooth/pulse = exposure-adaptation feedback limit cycle). Decrements
// per line; re-arm live by writing a new count via x64dbg.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_LumaWave = 300;
// CB-flap probe: the vrcam tonemap pass (the [2rt] bind) gets an 848-byte constant
// buffer uploaded right after its bind. Capture that CB resource+offset, read it back
// every frame alongside the luma sample, and auto-detect dwords that take EXACTLY two
// values and flip in sync with the bright/normal luma state -> that dword IS the raced
// exposure input, and its two bit-patterns identify the source variable in memory.
thread_local bool t_in_vrcam_2rt = false;   // between vrcam 2-RT bind and next OMSet
thread_local bool t_2rt_cb_armed = false;   // capture only the FIRST 848B upload
// There are SEVERAL vrcam 2-RT passes per frame; only the TONEMAP's RT1 descriptor
// ping-pongs across frames (persistent history pair), the others rebind the same RT1
// handle every frame. Identify the tonemap by observing an RT1 change for a given RT0
// (lock-free: hk_OM contains __try => no C++ destructors allowed => no mutexes here).
std::atomic<SIZE_T> g_2rt_seen_h0[4] = {};
std::atomic<SIZE_T> g_2rt_seen_h1[4] = {};
std::atomic<SIZE_T> g_tonemap_h0{0};
std::atomic<ID3D12Resource*> g_cb_res{nullptr};
std::atomic<uint64_t>        g_cb_off{0};
std::atomic<ID3D12Resource*> g_cb_last_res{nullptr};
std::atomic<bool>            g_cb_reset_pending{false};
ID3D12Resource* g_cb_rb[4] = {};     // READBACK, persistently mapped
uint8_t*        g_cb_map[4] = {};
bool            g_cb_valid[4] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCbCaptures = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFrameCb = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudCb = 0;
// Set by the composite's own validity check: the engine's b6 was bound and its target size
// matched ours, so every constant came from the engine rather than from the capture.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudCbUsed = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_Debug2RtBinds = 0;
ID3D12Resource* g_luma_rb[4] = {};      // READBACK heap, persistently mapped
uint8_t*        g_luma_map[4] = {};
uint32_t        g_luma_parity[4] = {};
uint32_t        g_luma_frame[4] = {};
uint32_t        g_luma_finidx[4] = {};   // fin natural index at capture (safe correlation)
bool            g_luma_valid[4] = {};
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugFinNatural;  // set in resolver
// mirror_stable_inline_copy is declared in Stereo/StereoInternal.hpp; its definition moved to another file.
// mirror_publish_output is declared in Stereo/StereoInternal.hpp; its definition moved to another file.
// Master gate for the OLD eye-X build path (Path 1a: own empty manager -> FinalOnly
// -> OOM). Disabled so `sync_typea_on` runs ONLY the new option-B context-inject
// (2nd context in main's manager) in isolation for testing.
bool g_enable_eye_x_build = false;
#ifdef TESTBED_EARLY_REG_PROBE
bool g_enable_native_registration_probe = true;
#else
bool g_enable_native_registration_probe = false;
#endif

// CALLER1 prologue, 22 fixed bytes (extracted from the live exe):
//   mov [rsp+10h],rbx ; push rbp/rsi/rdi ; lea rbp,[rsp-16B0h] ;
//   mov eax,17B0h ; call __chkstk
// The chkstk call displacement that follows the trailing E8 is position-
// dependent, so the pattern STOPS at the E8 opcode  everything before is
// invariant for this function.
const uint8_t kCaller1Pat[] = {
    0x48,0x89,0x5C,0x24,0x10, 0x55,0x56,0x57, 0x48,0x8D,0xAC,0x24,0x50,0xE9,0xFF,0xFF,
    0xB8,0xB0,0x17,0x00,0x00, 0xE8
};

const uint8_t kFlushRenderScenePat[] = {
    0x48,0x89,0x5C,0x24,0x20, 0x55,0x56,0x57, 0x48,0x8B,0xEC,
    0x48,0x81,0xEC,0x80,0x00,0x00,0x00, 0x48,0x8B,0xFA,
    0x48,0x8B,0xD9, 0xB2,0x01, 0x48,0x8D,0x4D,0xC8,
    0x45,0x33,0xC0, 0xE8
};

const uint8_t kCameraWritePat[] = {
    0x48,0x8B,0xC4, 0x48,0x89,0x58,0x08, 0x48,0x89,0x70,0x10,
    0x48,0x89,0x78,0x18, 0x4C,0x89,0x70,0x20, 0x55,
    0x48,0x8D,0xA8,0xE8,0xF4,0xFF,0xFF, 0x48,0x81,0xEC,0x10,0x0C
};

const uint8_t kFrameGatePat[] = {
    0x48,0x8B,0x49,0x20, 0xE9,0x7B,0x02,0x00,0x00
};

const uint8_t kFgBuildPat[] = {
    0x48,0x89,0x5C,0x24,0x08, 0x57,
    0x48,0x81,0xEC,0x80,0x00,0x00,0x00,
    0x4C,0x8B,0x4A,0x20, 0x48,0x8B,0xD9,
    0xBF,0x9A,0x01,0x00,0x00, 0x4D,0x85,0xC9,
    0x74,0x17, 0x65,0x48
};

const uint8_t kRunRenderNodesPat[] = {
    0x48,0x89,0x54,0x24,0x10, 0x48,0x89,0x4C,0x24,0x08,
    0x55,0x53,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57
};

const uint8_t kRunNodeBatchSubmitPat[] = {
    0x48,0x8B,0xC4, 0x48,0x89,0x58,0x08, 0x48,0x89,0x70,0x10,
    0x57, 0x48,0x83,0xEC,0x50, 0x83,0x60,0xF0,0x00
};

const uint8_t kRunNodeBatchWorkPat[] = {
    0x48,0x89,0x5C,0x24,0x08, 0x48,0x89,0x74,0x24,0x10,
    0x48,0x89,0x7C,0x24,0x18, 0x41,0x56, 0x48,0x81,0xEC,0x80,0x00,0x00,0x00
};

const uint8_t kGraphRequestBuildPat[] = {
    0x48,0x83,0xEC,0x58, 0x48,0x8B,0x51,0x28,
    0x4C,0x8D,0x89,0xD0,0x03,0x00,0x00,
    0x4C,0x8D,0x41,0x30, 0x0F,0x57,0xC0, 0x48,0x8B,0x49,0x20
};

const uint8_t kGraphContextPreparePat[] = {
    0x48,0x89,0x5C,0x24,0x18, 0x48,0x89,0x54,0x24,0x10,
    0x55,0x56,0x57, 0x41,0x54, 0x41,0x55, 0x41,0x56, 0x41,0x57,
    0x48,0x8D,0xAC,0x24,0x30,0xFC,0xFF,0xFF,
    0x48,0x81,0xEC,0xD0,0x04,0x00,0x00
};

const uint8_t kGraphRequestRegisterPat[] = {
    0x48,0x89,0x5C,0x24,0x10, 0x48,0x89,0x6C,0x24,0x18,
    0x56,0x57,0x41,0x56, 0x48,0x83,0xEC,0x30,
    0x48,0x8D,0x99,0x20,0x01,0x00,0x00,
    0x48,0x8B,0xEA, 0x4C,0x8D,0xB1,0x28
};


// CALLER1 ABI (from SYNC_SEQUENTIAL_PROVEN): rcx=mgr, xmm1=float, then ptrs.
// LIGHT (sub_14029A5B0) uses the identical ABI per Q1 of the round-6 audit.
using NodeDispatchFn = uint8_t (__fastcall *)(uintptr_t* node, uint8_t* work_context, void* args);

// Camera-writer sub_140788A9C (RVA 0x788A9C): writes view/proj into the
// view-state. Prologue 32 fixed bytes (no relative calls). Returns bool in al.
const uint8_t kCamwPat[] = {
    0x48,0x8B,0xC4, 0x48,0x89,0x58,0x08, 0x48,0x89,0x70,0x10, 0x48,0x89,0x78,0x18,
    0x4C,0x89,0x70,0x20, 0x55, 0x48,0x8D,0xA8,0xE8,0xF4,0xFF,0xFF, 0x48,0x81,0xEC,0x10,0x0C
};
using CamwFn = uint8_t (*)(void* rcx, void* rdx, void* r8, void* r9);

// View translation X lives at view-state +0x150 (camera space). Patching it
// laterally shifts the rendered image  the IPD lever (single float).
constexpr uintptr_t OFF_VIEW_TX = 0x150;

// mgr scalar offsets (round-6 SUBMIT mutation surface). +0x118 is the frame
// call counter; SUBMIT-fn (called by BOTH full and light) increments it.
constexpr uintptr_t OFF_FRAME_CTR = 0x118;  // dword
constexpr uintptr_t OFF_S_15C     = 0x15C;  // dword
constexpr uintptr_t OFF_S_188     = 0x188;  // dword
constexpr uintptr_t OFF_S_18C     = 0x18C;  // dword
// The node dispatcher and its observers moved to src/Stereo/NodeDispatch.cpp.


// --- Descriptor-heap probe / enlarge (Path A) ------------------------------
using PFN_D3D12CreateDevice = HRESULT (WINAPI*)(IUnknown*, D3D_FEATURE_LEVEL,
                                                REFIID, void**);
using PFN_CreateDescriptorHeap = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, const D3D12_DESCRIPTOR_HEAP_DESC*, REFIID, void**);

PFN_D3D12CreateDevice     g_orig_D3D12CreateDevice = nullptr;
PFN_CreateDescriptorHeap  g_orig_CreateDescriptorHeap = nullptr;
std::atomic<bool>         g_desc_probe_installed{false};
std::atomic<bool>         g_desc_vtable_patched{false};
// Enlarge the shader-visible CBV_SRV_UAV heap so two full views fit. RTX Tier 3
// allows >1,000,000 (bounded by VRAM). Kept modest; only applied to the big
// shader-visible heap. NOTE: on its own this only grows the real heap; the
// engine's internal budget still needs the matching site patch  this build is
// primarily to LOG the exact requesting site (caller RVA).
bool     g_enable_desc_heap_enlarge = false;
uint32_t g_desc_heap_target = 2000000u;

// Boot-time engine constant patch: sub_14091D604 builds the shader-visible
// CBV_SRV_UAV heap size as `0xDC240 - v4` (sub-allocator budget = heap+0x10) and
// NumDescriptors = budget + (v4 + 0x18000) = 0xDC240 + 0x18000 = 1,000,000.
// Raising the single base constant 0xDC240 -> 0x1DC240 scales BOTH the engine
// budget (868,928 -> 1,917,504) and the real D3D12 heap (1,000,000 -> 2,048,576)
// consistently, so two full views fit. RTX 5070 Ti is Resource Binding Tier 3.
// Instruction: 0x91D64A  B8 40 C2 0D 00  mov eax, 0xDC240  (imm32 at +1).
// DESC_HEAP_SIZE_ORIG moved to Stereo/StereoInternal.hpp.
// DESC_HEAP_SIZE_NEW moved to Stereo/StereoInternal.hpp.
// Disabled: raising the shader-visible CBV_SRV_UAV heap past 1,000,000 is
// rejected by the D3D12 runtime/driver (CreateDescriptorHeap returns null ->
// engine null-derefs at boot). Confirmed live: num=2,048,576 crashed. The 1M
// shader-visible cap is hard here; enlargement is not viable. Keep OFF.
bool     g_enable_desc_heap_resize = false;
std::atomic<bool>   g_desc_heap_resized{false};
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugDescHeapResized = 0;


// The device, queue, command-list and pipeline hooks moved to src/Stereo/DeviceHooks.cpp.

static HRESULT WINAPI Hook_D3D12CreateDevice(
        IUnknown* adapter, D3D_FEATURE_LEVEL fl, REFIID riid, void** out) {
    HRESULT hr = g_orig_D3D12CreateDevice(adapter, fl, riid, out);
    if (SUCCEEDED(hr) && out && *out) {
        {
            std::lock_guard<std::mutex> lock(g_game_object_mtx);
            if (!g_game_device) {
                auto* device = reinterpret_cast<ID3D12Device*>(*out);
                device->AddRef();
                g_game_device = device;
                log("[mirror] captured real game device=%p", device);
            }
        }
        patch_device_descriptor_slot(*out);
    }
    return hr;
}

static bool g_desc_ring_probe_installed = false;

// Inject a 2nd (eye) context into a RENDER manager's map, called from the
// GraphContextPrepare hook (its `manager` arg is a real render manager). We clone
// the manager's OWN main context (key-0) view-params at ctx+7712, IPD-shift the
// camera, and create an eye-keyed context via sub_14036FD10 so  if the prepare
// then rebuilds its active list (+0x48) from the map  the FG loop builds the eye.
// Register the eye via the ENGINE'S OWN registration QUEUE (sub_142906A28), the
// same path the mirror/RTT use  this creates a properly set-up context (a5=1 via
// sub_14079AA1C) that GraphContextPrepare then appends to the active list. We build
// an eye request = main ctx's view-params (IPD-shifted camera) + distinct key @944
// + mode 0 (full) @976 + zeroed extras. sub_142906A28 fills mgr+296; the g_orig
// GraphContextPrepare that runs right after consumes it and builds the eye.
// --- RTT-camera view-create resolution override (C2: dynamic resolution) ---
// sub_1404FBAFC(a1=RTT component, a2) creates the offscreen view; it reads the view dims from
// comp+0x258 (width) / comp+0x25C (height). VRCAM bakes 1600x900. We overwrite those with a
// dynamic resolution just before the engine reads them => the engine creates a FULLY-registered
// view at OUR resolution (no cascade crashes). Component identified by render-host vtable RVA
// 0x307BFD0 + the VRCAM's baked 1600x900.
using RttViewCreateFn = char (__fastcall*)(__int64, __int64);
RttViewCreateFn g_orig_rtt_viewcreate = nullptr;
// OFF by default: the VRCAM asset texture is now authored at the target
// resolution (e.g. 2444x2444), and the RT-activation derives the view dims
// from the output texture. Forcing 1222 here SHRANK the render and meant no
// 2444^2 resource ever existed => dump_rt found nothing. Leave the hook
// installed (so we can flip it live via IPC) but pass-through by default.
bool g_rtt_res_override = false;
uint32_t g_rtt_w = 1222;   // only used when g_rtt_res_override is toggled on
uint32_t g_rtt_h = 1222;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRttHits = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRttComp = 0;
// Extra debug exports so x64dbg can read the live RTT component + the dims the
// view-create actually receives, regardless of the (default-off) override.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugRttW = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugRttH = 0;

// --- Dynamic RTT resize via engine ResizeDynamicTexture (match MAIN, no assets) ---
// The vrcam RENDER size = the OUTPUT DynamicTexture size (*(comp+0x1E8) ->
// dtex, width@0x40/height@0x44), NOT comp resolutionWidth/Height (that only
// affects view-create dims, the render follows the texture). So resize the dtex
// itself via the engine's thread-marshaled ResizeDynamicTexture (texMgr vtable[80]
// = sub_14291A4D4). texMgr = *(*(exe+0x3427C00)+0x70). Signature proven live:
//   char f(rcx=texMgr, rdx=&dtexPtr, r8d=w, r9d=h, [rsp20]=flag).
// Target = explicit CyberpunkVR_RttResizeW/H, else MAIN's render dims (g_main_ctx
// +0x44 W / +0x4C H). Default OFF (enable live via x64dbg to verify no crash).
using ResizeDynTexFn = char (__fastcall*)(void*, void**, uint32_t, uint32_t, int);
ResizeDynTexFn g_resize_dyntex = nullptr;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_RttResizeMatchMain = 0; // 1=resize dtex -> target
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_RttResizeW = 0;         // explicit W (0=use main)
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_RttResizeH = 0;         // explicit H (0=use main)
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRttResizeHits = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugRttDtexW = 0;      // current dtex W (live)
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugRttDtexH = 0;
// The bound VRCAM component. Resolved ONCE (by the selected resolution) and then reused, so
// the per-frame writes never have to re-decide which component they are talking to.
std::atomic<uintptr_t> g_vrcam_comp{0};
// Its AUTHORED fov, captured at bind before anything of ours writes to it.
float g_vrcam_base_fov = 0.f;
extern "C" __declspec(dllexport) float    CyberpunkVR_DebugVrcamBaseFov = 0.f;
// Components skipped because their dims are not the selected resolution. Non-zero here is
// normal: it counts the ones we correctly refused to point the fov writes at.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRttCompRejects = 0;

// The frame-graph observation and the RTT view moved to src/Stereo/FrameGraph.cpp.

// --- Distant-shadow reuse for VRCAM --------------------------------------
// CRenderNode_RenderDistantShadows work sub_140373998 gates its ENTIRE body on
// feature bit 11 (0x800 @ctx+0x17D0). Unlike GI (bit 31 gates only the update
// sub_14077F758, apply runs regardless = clean reuse) distant has NO reuse-only
// sub-gate: clearing bit 11 removes vrcam's distant shadows; setting it makes
// vrcam ADVANCE the SHARED distant manager (*(node_data+160), incremental ~1Hz
// slice state machine via sub_140374B48) -> desyncs main's incremental update ->
// ~1Hz sun/shadow shift. Reuse = keep bit 11 SET (vrcam's lighting still samples
// the distant map) but SKIP vrcam's distant node entirely so it neither advances
// the shared manager nor re-renders -> vrcam reuses main's distant result while
// main stays clean. Work-fn ABI: a2=rdx, view ctx = *(a2+0x18), key @ctx+0x28.
// Both distant nodes advance the SHARED distant manager (*(node_data+160)) via
// sub_140374B48: RenderDistantShadows sub_140373998 AND PrepareDistantShadows
// sub_140374AD8. Skip BOTH for vrcam so it never touches the shared state.
// Moved to src/Stereo/ViewReuse.cpp: the amortised sky.
// Moved to src/Stereo/Grading.cpp: colour grading for the second eye.

// Colour grading and tonemapping moved to src/Stereo/Grading.cpp.

using GiNodeFn = char(__fastcall*)(void*, void*);
GiNodeFn g_orig_gi_node = nullptr;
// Moved to src/Stereo/ViewReuse.cpp: the GI reuse mode.
// Moved to src/Stereo/CullVisibility.cpp: cull reuse, the gather context, and the LOD sweep.

// Culling, visibility and the prepare stages moved to src/Stereo/CullVisibility.cpp.

// --- Block-list (v5) reuse at DrawComposition layer -----------------------
// DrawComposition sub_14020A264 resolves viewData = sub_1401ED930(a2) and reads
// v5 = *(viewData + 0x168). If the scene/materialize path is enabled, it later calls
// sub_1401ECFDC(pool, v5). Live test proved: forcing VRCAM's materialize to use MAIN's
// v5 renders the SCENE correctly (no dome). The clean fix is to inject earlier: on VRCAM,
// if v5 is empty or VRCAM-like, temporarily replace viewData+0x168 with cached MAIN v5,
// call original DrawComposition, then restore. Frame order is VRCAM-first, so this uses
// MAIN's PREVIOUS-FRAME v5.
using DrawCompFn = char(__fastcall*)(void*, void*);
DrawCompFn g_orig_drawcomp = nullptr;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBlockV5ReuseHits = 0;
void* g_main_block_v5 = nullptr;

// Moved to src/Stereo/ViewReuse.cpp: lending MAIN's draw-block list.

// --- VRCAM temporal-history fix (ROOT CAUSE of no-SSR/GI/denoise) ----------
// CRenderNode_SetStreamlineConstants (sub_140788A9C) does the per-view
// current-to-history matrix copy (sub_14078933C) + Streamline constants that
// feed ALL temporal reprojection (SSR feedback, GI/SSGI history, REBLUR shadow/
// AO denoise, TAA). It is gated at entry on the view's AA/upscaler mode field:
//   obj = ([[a2]] vtable[+0x20])();  mode = *(uint32*)(obj + 0xF94);
//   proceeds only if mode == 0 (TAA) or 4 (DLSS); else EXITS before the copy.
// MAIN's view = mode 0 -> history maintained. VRCAM's RTT view = mode 1 (native/
// no-AA) -> node exits -> vrcam never accumulates any temporal history -> broken
// reflections, wrong/leaking light, triangular denoiser shadows. PROVEN live:
// forcing vrcam's mode to a temporal value makes the node run the history copy.
// Fix: MIRROR main's AA/upscaler mode onto vrcam (not hardcoded) so vrcam follows
// whatever main uses (0=TAA, 4=DLSS, ...) and DLSS parity is possible later. The
// node runs for BOTH views each frame; we observe main's mode (key==0) and apply
// it to vrcam. Frame order is VRCAM-first so vrcam uses last frame's main mode
// (mode changes rarely). Default 0 (TAA) until main is first observed.
using SlConstFn = __int64(__fastcall*)(void*, void*, void*);
SlConstFn g_orig_sl_const = nullptr;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_StreamlineHistoryFix = 1;   // default ON
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugSlHistoryHits = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMainAaMode = 0;   // observed main AA mode  *(view+0xF94)
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugVrcamAaMode = 0;   // vrcam AA mode (pre-fix) *(view+0xF94)
// The framegraph builder-selection in sub_140219730 dispatches on the view's
// BUILD-mode = *(view+0xF90) (v93): <2 -> full gameplay (sub_141D43040, emits
// StartRender/ExtractionFinalColor/ClearFinalColorTarget/Present), ==2 HitProxy,
// ==4 -> sub_141D47FF0 (full scene, NO final-color/present), etc. This is the
// field that actually gates the "main-only" output nodes -- NOT the AA mode we
// already mirror (0xF94). Capture both live in the always-running SL hook.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMainBuildModeF90  = 0xFFFFFFFF; // main  *(view+0xF90)
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugVrcamBuildModeF90 = 0xFFFFFFFF; // vrcam *(view+0xF90)

// Moved to src/Stereo/Dlss.cpp: the vrcam Streamline viewport knobs.
// POST-DLSS CROP FIX phase flag: 0 = vrcam SCENE phase (render-res 1418, DLSS input),
// 1 = vrcam POST phase (this frame's DLSS eval already ran -> remaining vrcam passes are
// OUTPUT-domain 2444). Set at end of vrcam's DLSS eval; reset each frame at main's ApplyDLSS
// (which runs once per frame, before the vrcam mirror view, by frame-graph dependency).
// Root cause (verified live): the render-res writer sub_1404E42A0 writes the render-res sub-
// struct (VP+0x34 renderW / VP+0x38 renderH + copies) that ALL of a view's raster passes read
// for their D3D viewport (via sub_1401F6350 -> RSSetViewports). We force the DLSS flag ON for
// the WHOLE vrcam frame (needed for anti-flicker source-selection), so g_orig scales that field
// to 1418 on EVERY writer call -> even post-DLSS passes get a 1418 viewport on the 2444 output
// = crop. MAIN never scales its post passes (its post-DLSS passes are output-domain). Fix:
// decouple the flag from the render-res VALUE -- keep the flag set, but in the post phase make
// our own override write the TARGET (2444) instead of the scaled 1418.
// (removed: dead vrcam_dlss_force_dims / vrcam_dlss_restore_dims + DLSS_*_DIMS_OFF + g_vrcam_eval_done.
//  The DLSS-subrect force was never called; vrcam's DLSS subrect is correct natively -- see the
//  "ATTEMPT 3 DISPROVEN" note above. g_vrcam_dlss_rw/rh/ow/oh remain, published by Detour_RenderRes.)

// getter-entry hook: force vrcam POST resources to output-domain (see block above).
//   mode 1 = CASE C: clear gate + node dims -> getter falls through to renderer+0x148
//            (global output). VR-correct (global == HMD per-eye == 2444) but on DESKTOP
//            global == window res (1920) != 2444 texture -> 3-res mismatch artifacts.
//   mode 2 = CASE B: clear gate, but SET node dims (ctx+0x2C word=W, +0x2E word=H) to
//            vrcam's OWN output (VP+0x54 / VP+0x58 = 2444) -> getter returns 2444 directly,
//            matching the DLSS output + RTT texture in BOTH desktop and VR (no global dep).

// The capture path moved to src/Stereo/Capture.cpp.

// One tiny copy submit on the GAME queue: dtex (rests permanently in RENDER_TARGET)
// -> g_d12_mtex. Called once per vrcam frame from ExecuteCommandLists right after
// the game submits the vrcam blit list, so on the queue timeline it runs after the
// blit and reads the freshly rendered frame. A 4-slot allocator ring is reused
// without stalling the game: if a slot's copy is still in flight, skip this mirror
// frame instead of waiting. Uses raw vtable barrier/copy to avoid re-entering the
// hooked ResourceBarrier, and g_orig_ExecuteCommandLists to bypass our own hook.
ID3D12CommandAllocator*    g_d12_copy_alloc[4] = {};
ID3D12GraphicsCommandList* g_d12_copy_list = nullptr;
uint64_t                   g_d12_copy_slot_fence[4] = {};
uint32_t                   g_d12_copy_frame = 0;
std::mutex                 g_d12_copy_mtx;

// The luma / constant / tonemap-input probes moved to src/Stereo/LumaProbe.cpp.

// Moved to src/Stereo/Mirror.cpp: the submit path.

// hk_ResourceBarrier moved to src/Stereo/Capture.cpp: it tracks the state a resource rests in.

// Nsight cannot capture this setup, so the mod has to answer the question a capture would:
// what does each view actually upload into the light path? Summing the bytes was too blunt --
// it mixed every buffer the two nodes touch and produced a meaningless 0.86 ratio. A HISTOGRAM
// of upload sizes separates them: the light array is one distinctive size, and if VRCAM's copy
// of that size is smaller, or missing, that is the answer.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_LightCensus = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLightBytesMain    = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLightBytesVrcam   = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLightUploadsMain  = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLightUploadsVrcam = 0;

// The histogram found it: an 848-byte constant upload into a 1024-byte buffer, MAIN 1500 times
// and VRCAM never, while every real light array is uploaded equally by both. The question that
// decides the fix is whether VRCAM SHARES that buffer (then it reads MAIN's constants and this
// is a red herring) or owns an instance nothing ever fills (then it reads zeros). So record the
// destination resources each view touches, not just the sizes.
// struct LightDst now lives in Stereo/StereoInternal.hpp: the census indexes the array below.
std::array<LightDst, 24> g_light_dsts{};
uint32_t g_light_dst_n = 0;

// The command-list census and the probes it feeds moved to src/Stereo/CommandListCensus.cpp.


// Moved to src/Stereo/Mirror.cpp: the window and the present thread.


// Resolve the view-output ctx from the node arg via the engine's own vcall
// (obj = ([[a2]] vtable[+0x20])()), returning &obj[0xF94] (AA/upscaler mode).
 uintptr_t sl_view_obj(void* a2) {
    uintptr_t X = *reinterpret_cast<uintptr_t*>(a2);            // *(a2)
    if (!X) return 0;
    uintptr_t vt = *reinterpret_cast<uintptr_t*>(X);           // X's vtable
    using GetViewFn = uintptr_t(__fastcall*)(uintptr_t);
    GetViewFn getv = *reinterpret_cast<GetViewFn*>(vt + 0x20);
    return getv(X);                                            // view-state object
}

// The DLSS / Streamline detours moved to src/Stereo/Dlss.cpp.
static void install_desc_ring_probe() {
    if (g_desc_ring_probe_installed) return;
    if (!g_exe_base) sync_stereo_init();
    MH_STATUS st = MH_Initialize();
    if (st != MH_OK && st != MH_ERROR_ALREADY_INITIALIZED) {
        log("[install] MH_Initialize failed: %d", (int)st); return;
    }
    // Everything that has moved to the registry, installed in one pass, ascending RVA.
    InstallEngineDetours();

    // Frame-graph build observers (DLSS upscaler-group capture/force backup for the crop fix).
    // full-build and incr-build are registered at the bottom of this file; see
    // Stereo/DetourRegistry.hpp for why the mention now lives with the detour.
    g_handle_assign = reinterpret_cast<HandleAssignFn>(g_exe_base + HANDLE_ASSIGN_RVA);
    // DLSS-for-vrcam group force + crop fix (view+0x17D0).
    // flag-compute and rect-compute: registered, as above.
    // VRCAM reuse optimizations (distant shadows / local shadows / GI).
    {
        void* cnd = reinterpret_cast<void*>(g_exe_base + CLOUDS_NODE_RVA);
        if (MH_CreateHook(cnd, (void*)&Detour_CloudsNode, (void**)&g_orig_clouds_node) == MH_OK &&
            MH_EnableHook(cnd) == MH_OK)
            log("[cloudnode] clouds node sub_14061B5B4 hooked @%p", cnd);
        else
            log("[cloudnode] failed to hook the clouds node @%p", cnd);
    }
    {
        void* skw = reinterpret_cast<void*>(g_exe_base + SKY_WORK_RVA);
        if (MH_CreateHook(skw, (void*)&Detour_SkyWork, (void**)&g_orig_sky_work) == MH_OK &&
            MH_EnableHook(skw) == MH_OK)
            log("[sky] sky work sub_1407818F8 hooked @%p (SkyReuseMode=%u)",
                skw, CyberpunkVR_SkyReuseMode);
        else
            log("[sky] failed to hook sky work @%p -- both views keep filling one sky", skw);
    }
    // Detour_DistantRender -> registered at the bottom of this file.
    // Detour_DistantPrepare -> registered at the bottom of this file.
    // Detour_LightBuffers -> registered at the bottom of this file.
    // Detour_CloudCbFill -> registered at the bottom of this file.
    // Detour_LocalShadowMaps -> registered at the bottom of this file.
    // Detour_CbUpload -> registered at the bottom of this file.
    // Detour_GradingCompose -> registered at the bottom of this file.
    // Detour_TonemapLut -> registered at the bottom of this file.
    // Detour_GiNode -> registered at the bottom of this file.
    // Detour_GraphContextPrepare -> registered at the bottom of this file.
    // Detour_GraphContextReset -> registered at the bottom of this file.
    // Detour_VisibilityCollector -> registered at the bottom of this file.
    // Detour_MaterializeWorker -> registered at the bottom of this file.
    // Detour_MainCullPrepare -> registered at the bottom of this file.
    // Detour_GatherCtxInit -> registered at the bottom of this file.
    // Detour_VisQueryPrepare -> registered at the bottom of this file.
    // Detour_MainCullTest -> registered at the bottom of this file.
    // Detour_FineMaterialize -> registered at the bottom of this file.
    // Detour_VisibleAppend -> registered at the bottom of this file.
    // Detour_PrepareStage -> registered at the bottom of this file.
    // Detour_PrepareGather -> registered at the bottom of this file.
    // Detour_PrepareFilter -> registered at the bottom of this file.
    // Detour_PrepareFinalize -> registered at the bottom of this file.
    // Detour_PrepareSortA -> registered at the bottom of this file.
    // Detour_PrepareSortB -> registered at the bottom of this file.
    // Detour_PrepareSortC -> registered at the bottom of this file.
    // Detour_PrepareSortFinal -> registered at the bottom of this file.
    // Detour_DoCulling -> registered at the bottom of this file.
    // Detour_QueryWork -> registered at the bottom of this file.
    // Detour_DrawComposition -> registered at the bottom of this file.
    // Stereo/IPD + temporal-history via SetStreamlineConstants.
    // Detour_SlConstants -> registered at the bottom of this file.
    // DLSS-for-vrcam drivers (own Streamline viewport + eval + apply).
    // Detour_DlssConst -> registered at the bottom of this file.
    // Detour_DlssEval -> registered at the bottom of this file.
    // Detour_ApplyDlss -> registered at the bottom of this file.
    // DLSS-upscale render-res scaler + crop fix (view+0x17D0 match-main).
    // Detour_RenderRes -> registered at the bottom of this file.
    // Node dispatcher: drives mirror-copy epilogue + flicker tonemap snapshot + t_current_node_work.
    if (!g_node_dispatch_hooked.load(std::memory_order_acquire)) {
        void* nd = reinterpret_cast<void*>(g_exe_base + NODE_DISPATCH_RVA);
        const MH_STATUS ndSt = MH_CreateHook(nd, (void*)&Detour_NodeDispatch,
                                             (void**)&g_node_dispatch_orig);
        if (ndSt == MH_OK && MH_EnableHook(nd) == MH_OK) {
            g_node_dispatch_hooked.store(true, std::memory_order_release);
            // The proxy reads the view identity from CyberpunkVR_IsMainViewActive(), which
            // this detour maintains, and skips hooking this address itself.
            log("[node] node dispatcher sub_1401EC404 hooked @%p (view key published)", nd);
        } else {
            // Name the status. ALREADY_CREATED means something else took this address first,
            // and the consequence is total: no vrcam node tagging, so no RTV capture, no
            // snapshot, no right eye and no mirror window. That is worth more than "failed".
            log("[node] failed to hook node dispatcher @%p (MH_STATUS=%d%s)", nd, (int)ndSt,
                ndSt == MH_ERROR_ALREADY_CREATED ? " ALREADY_CREATED -- another hook owns it" : "");
        }
    }
    // Per-view capability test -- the gate that keeps the HUD out of the second eye.
    // Hooked unconditionally; the detour itself is a no-op unless CyberpunkVR_HudInVrcam is on
    // AND the second eye is the current view, so leaving it installed costs two loads per call.
    {
        void* vf = reinterpret_cast<void*>(g_exe_base + VIEW_FEATURE_CHECK_RVA);
        const MH_STATUS st = MH_CreateHook(vf, (void*)&Detour_ViewFeatureCheck,
                                           (void**)&g_view_feature_check_orig);
        if (st == MH_OK && MH_EnableHook(vf) == MH_OK) {
            log("[hud] view capability test sub_14021BE28 hooked @%p (HudInVrcam=%d)",
                vf, CyberpunkVR_HudInVrcam);
        } else {
            log("[hud] failed to hook view capability test @%p (MH_STATUS=%d) -- the second eye "
                "keeps the engine's HUD refusal", vf, (int)st);
        }
    }
    // RTT view-create (mirror serial + RTT res) and post-color de-alias (flicker fix).
    // Detour_RTTViewCreate -> registered at the bottom of this file.
    // Detour_Resolve3D20 -> registered at the bottom of this file.
    g_desc_ring_probe_installed = true;
    log("[install] engine hooks installed");
}

// prof_log_config moved to src/Stereo/Profiler.cpp, where the configuration it prints lives.

}  // namespace detail
using namespace detail;

// The engine's HUD composite constants, for the eye path in openxr_capture.cpp. Defined out here
// because everything above lives in an anonymous namespace and so has internal linkage.
ColorBlit::HudParams CyberpunkVR_GetHudParams() { return hud_composite_params(); }

void sync_stereo_ensure_descriptor_probe() {
    patch_descriptor_heap_size();
    bool expected = false;
    if (!g_desc_probe_installed.compare_exchange_strong(expected, true)) return;
    if (!g_exe_base) sync_stereo_init();
    HMODULE d3d12 = GetModuleHandleW(L"d3d12.dll");
    if (!d3d12) d3d12 = LoadLibraryW(L"d3d12.dll");
    if (!d3d12) { log("[descheap] d3d12.dll not present"); return; }
    void* target = reinterpret_cast<void*>(GetProcAddress(d3d12, "D3D12CreateDevice"));
    if (!target) { log("[descheap] D3D12CreateDevice not found"); return; }
    MH_STATUS st = MH_Initialize();
    if (st != MH_OK && st != MH_ERROR_ALREADY_INITIALIZED) {
        log("[descheap] MH_Initialize failed: %d", (int)st); return;
    }
    if (MH_CreateHook(target, (void*)&Hook_D3D12CreateDevice,
                      (void**)&g_orig_D3D12CreateDevice) != MH_OK ||
        MH_EnableHook(target) != MH_OK) {
        log("[descheap] failed to hook D3D12CreateDevice @%p", target);
        return;
    }
    log("[descheap] D3D12CreateDevice hook installed @%p (enlarge=%d target=%u)",
        target, (int)g_enable_desc_heap_enlarge, g_desc_heap_target);
}


void sync_stereo_init() {
    load_vrcam_selection();     // before any hook can observe a view
    // Adopts the enabled component's REAL virtualCameraName once CET reports it, so a
    // component whose camera field does not follow the naming convention still works.
    std::thread(vrcam_active_watcher).detach();
    g_exe_base = (uint8_t*)GetModuleHandleW(nullptr);
    QueryPerformanceFrequency(&g_qpc_freq);
    if (g_qpc_freq.QuadPart) g_qpc_to_ms = 1000.0 / (double)g_qpc_freq.QuadPart;
    HMODULE kernelbase = GetModuleHandleW(L"kernelbase.dll");
    g_wait_on_address = reinterpret_cast<WaitOnAddressFn>(
        GetProcAddress(kernelbase, "WaitOnAddress"));
    g_wake_by_address_all = reinterpret_cast<WakeByAddressAllFn>(
        GetProcAddress(kernelbase, "WakeByAddressAll"));
}

void sync_stereo_install_early_hooks() {
    patch_descriptor_heap_size();
    // Install before the game's D3D12CreateDevice so the real device, DIRECT queue and
    // command-list vtables are captured without creating a dummy device through Streamline.
    // As the DXGI proxy we are loaded long before the game touches d3d12, so "early" comes
    // for free here -- in the standalone plugin this was the delicate part.
    sync_stereo_ensure_descriptor_probe();
    install_desc_ring_probe();
    // No overlay install: the plugin build had to raise its own ImGui overlay on a dummy
    // swapchain because a red4ext plugin cannot see Present any other way. The proxy hooks
    // Present directly and draws the panel itself (overlay/imgui_overlay.cpp, Stereo tab).
}


} // namespace cvr
namespace cvr {
namespace detail {

// ---- the first four detours to declare themselves ---------------------------------------------
//
// The other forty-two are still installed by hand in install_desc_ring_probe above; they move here
// one at a time, and each move is a line added below and five lines deleted there.

// ---- and the rest of the plain ones -----------------------------------------------------------
//
// Converted mechanically: every site whose shape was exactly "resolve the RVA, MH_CreateHook,
// MH_EnableHook, log either way" and nothing else. The nine that are NOT here are gated by an `if`
// or compute their target differently, and each needs a CVR_DETOUR_IF or a decision of its own --
// converting them by pattern would have dropped the condition, which is the mistake this
// restructure has already made once with PatchBuffer and does not intend to repeat.
CVR_DETOUR("[flicker] post-color de-alias sub_1401F3D20", RESOLVE_QUERY_RVA, Detour_Resolve3D20, g_orig_resolve3d20)

}  // namespace detail
}  // namespace cvr
