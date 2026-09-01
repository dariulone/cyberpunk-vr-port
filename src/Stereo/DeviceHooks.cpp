// DeviceHooks -- how everything else in this module gets to run at all.
//
// The engine creates a D3D12 device, a queue, command lists and pipeline states. This file hooks the
// creation of each, so that by the time any other file here needs a command list to be watched, it
// already is.
//
// patch_command_list_vtable IS THE COUPLING THIS MODULE HAS NOT YET BROKEN. It installs TWELVE slots
// whose hooks are defined in three other files -- counted, not estimated:
//
//   CommandListCensus.cpp   7   slots 12, 13, 14, 15, 25, 44, 59  (draw, dispatch, copy, PSO, VB)
//   Dlss.cpp                3   slots 10, 21, 22                  (reset, viewport, scissor)
//   Capture.cpp             2   slots 26, 46                      (barrier, render targets)
//
// A vtable slot has no registry, so this function has to name every one of them -- precisely the
// coupling CVR_DETOUR removed for the RVA detours. A slot registry is the obvious next move; until it
// exists those hooks are declared in Stereo/StereoInternal.hpp and named here.
//
// (The mirror's appended copy is NOT in that list. It is called from Hook_ExecuteCommandLists, not
// installed into a slot, which is why it needed no declaration when the mirror moved out.)
//
// WHY THE DESCRIPTOR HEAP IS RESIZED. patch_descriptor_heap_size and Hook_CreateDescriptorHeap exist
// because the second view needs descriptors the engine did not budget for. Growing the heap at creation
// is the only place it can be done without owning the allocator.
//
// The pipeline-state hooks are not decoration either: identifying a draw by its PIXEL SHADER is what
// made the outline's blend mode findable, and pso_stream_find is how a shader is recognised inside a
// stream description whose layout the SDK does not expose.
//
// Everything here is a creation-time hook, so all of it runs before the frame loop and none of it is
// on a per-frame path. That is the reason this file can afford to log as much as it does.

#include "Stereo/SyncStereo.hpp"
#include "Utils/StereoLog.hpp"
#include "Stereo/VrcamConfig.hpp"   // vrcam.json access + CName hashing, shared with the launcher
#include "Render/ColorBlit.hpp"   // HUD debug overlay on the mirror image
#include <windows.h>
#include <d3d12.h>
#include "Hooks/SwapChainInternal.hpp"   // g_dredDevice -- the device, taken from the swapchain   // ID3D12InfoQueue, for the drain below
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

void patch_descriptor_heap_size() {
    if (!g_enable_desc_heap_resize) return;
    bool expected = false;
    if (!g_desc_heap_resized.compare_exchange_strong(expected, true)) return;
    if (!g_exe_base) sync_stereo_init();
    uint8_t* mov = g_exe_base + DESC_HEAP_SIZE_MOV_RVA;
    __try {
        if (mov[0] != 0xB8 ||
            *reinterpret_cast<uint32_t*>(mov + 1) != DESC_HEAP_SIZE_ORIG) {
            CyberpunkVR_DebugDescHeapResized = 0xFFFFFFFFu; // validation failed
            log("[descheap] size-const validation FAILED at %p (op=0x%02X imm=0x%X)",
                mov, mov[0], *reinterpret_cast<uint32_t*>(mov + 1));
            return;
        }
        DWORD oldp = 0;
        if (!VirtualProtect(mov + 1, 4, PAGE_EXECUTE_READWRITE, &oldp)) return;
        *reinterpret_cast<uint32_t*>(mov + 1) = DESC_HEAP_SIZE_NEW;
        DWORD junk = 0;
        VirtualProtect(mov + 1, 4, oldp, &junk);
        FlushInstructionCache(GetCurrentProcess(), mov, 5);
        CyberpunkVR_DebugDescHeapResized = DESC_HEAP_SIZE_NEW;
        log("[descheap] heap size const patched 0x%X -> 0x%X at %p",
            DESC_HEAP_SIZE_ORIG, DESC_HEAP_SIZE_NEW, mov);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        CyberpunkVR_DebugDescHeapResized = 0xEEEEEEEEu;
    }
}

static HRESULT STDMETHODCALLTYPE Hook_CreateDescriptorHeap(
        ID3D12Device* self, const D3D12_DESCRIPTOR_HEAP_DESC* desc,
        REFIID riid, void** out) {
    const uintptr_t ret_abs = reinterpret_cast<uintptr_t>(_ReturnAddress());
    CyberpunkVR_DebugDescHeapCreates++;
    D3D12_DESCRIPTOR_HEAP_DESC local;
    const D3D12_DESCRIPTOR_HEAP_DESC* use = desc;
    if (desc) {
        const bool shader_visible =
            (desc->Flags & D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE) != 0;
        if (desc->Type == D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV &&
            shader_visible && desc->NumDescriptors >= 0x40000u) {
            uintptr_t rva = ret_abs - reinterpret_cast<uintptr_t>(g_exe_base);
            CyberpunkVR_DebugDescHeapSVNum = desc->NumDescriptors;
            CyberpunkVR_DebugDescHeapSVRetRva = rva;
            CyberpunkVR_DebugDescHeapSVRetAbs = ret_abs;
            CyberpunkVR_DebugDescHeapSVFlags = desc->Flags;
            log("[descheap] shader-visible CBV_SRV_UAV num=%u flags=0x%X caller_abs=0x%llX rva=0x%llX",
                desc->NumDescriptors, desc->Flags,
                (unsigned long long)ret_abs, (unsigned long long)rva);
            if (g_enable_desc_heap_enlarge && desc->NumDescriptors < g_desc_heap_target) {
                local = *desc;
                local.NumDescriptors = g_desc_heap_target;
                use = &local;
                CyberpunkVR_DebugDescHeapEnlarged++;
                log("[descheap] enlarged -> %u", g_desc_heap_target);
            }
        }
    }
    return g_orig_CreateDescriptorHeap(self, use, riid, out);
}

// --- ExecuteCommandLists probe: count actual GPU command-list executions per
// frame. Definitive test for "does the eye execute?": if execLists/frame ~doubles
// under stereo, the eye's command lists really run on the GPU. Queue vtable slot
// 10 = ExecuteCommandLists; device vtable slot 8 = CreateCommandQueue.
bool g_enable_exec_probe = true;
std::atomic<uint64_t> g_exec_total{0};        // cumulative command lists executed
std::atomic<uint64_t> g_exec_last_frame{0};
std::atomic<uint64_t> g_exec_frame_ctr{0};
std::atomic<bool>     g_queue_vtable_patched{false};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugExecTotal    = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugExecPerFrame = 0;

using PFN_ExecuteCommandLists =
    void (STDMETHODCALLTYPE*)(ID3D12CommandQueue*, UINT, ID3D12CommandList* const*);
using PFN_CreateCommandQueue =
    HRESULT (STDMETHODCALLTYPE*)(ID3D12Device*, const D3D12_COMMAND_QUEUE_DESC*, REFIID, void**);
PFN_ExecuteCommandLists g_orig_ExecuteCommandLists = nullptr;
PFN_CreateCommandQueue  g_orig_CreateCommandQueue  = nullptr;
using PFN_CreateCommandList = HRESULT (STDMETHODCALLTYPE*)
    (ID3D12Device*, UINT, D3D12_COMMAND_LIST_TYPE, ID3D12CommandAllocator*,
     ID3D12PipelineState*, REFIID, void**);
using PFN_ResourceBarrier = void (STDMETHODCALLTYPE*)
    (ID3D12GraphicsCommandList*, UINT, const D3D12_RESOURCE_BARRIER*);
using PFN_CopyResource = void (STDMETHODCALLTYPE*)
    (ID3D12GraphicsCommandList*, ID3D12Resource*, ID3D12Resource*);
using PFN_OMSetRenderTargets = void (STDMETHODCALLTYPE*)
    (ID3D12GraphicsCommandList*, UINT, const D3D12_CPU_DESCRIPTOR_HANDLE*,
     BOOL, const D3D12_CPU_DESCRIPTOR_HANDLE*);
// Slot 14. Used only to NAME the node behind a dispatch: the light-tile pass was identified in
// a capture by its group count, but a capture cannot say which frame-graph node issued it.
using PFN_Dispatch = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*, UINT, UINT, UINT);
// hk_Dispatch moved with the census; declared in Stereo/StereoInternal.hpp.

using PFN_ExecuteIndirect = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,
    ID3D12CommandSignature*, UINT, ID3D12Resource*, UINT64, ID3D12Resource*, UINT64);
// hk_ExecuteIndirect moved with the census; declared in Stereo/StereoInternal.hpp.

// Direct draws, vtable slots 12 and 13. The dispatch census (slot 14) and the indirect census
// (slot 59) between them still miss ordinary geometry, which is what per-object effects like the
// scanner's object highlight are drawn with -- so "which nodes draw for MAIN and never for VRCAM"
// was unanswerable. Same shape as the other two censuses: aggregate per node, report the
// asymmetry, never bin by argument values (that produced a false lead once already).
using PFN_DrawInstanced = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,
    UINT, UINT, UINT, UINT);
using PFN_DrawIndexedInstanced = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,
    UINT, UINT, UINT, INT, UINT);
// hk_DrawInstanced moved with the census; declared in Stereo/StereoInternal.hpp.
// Slot 25. Needed only to know WHICH pso a draw runs under: the holographic sight's reticle is
// one specific pixel shader, and a shader can only be substituted at PSO-creation time, so the
// creation site has to be told which one to substitute. Identity travels as the PS bytecode
// hash, which is stable across runs; the pointer is not.
using PFN_SetPipelineState = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,
    ID3D12PipelineState*);
// hk_SetPipelineState moved with the census; declared in Stereo/StereoInternal.hpp.
// Slot 44. The sight quad's placement rides in the instance stream at slot 7; reading it at the
// draw is the only way to compare the weapon's ORIENTATION between the two views, which is now
// the one remaining input that can differ. (The collimated direction is eye-position independent
// by construction, so a per-eye disagreement can only come from per-view instance data.)
using PFN_IASetVertexBuffers = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,
    UINT, UINT, const D3D12_VERTEX_BUFFER_VIEW*);
// hk_IASetVertexBuffers moved with the census; declared in Stereo/StereoInternal.hpp.
// hk_DrawIndexedInstanced moved with the census; declared in Stereo/StereoInternal.hpp.

using PFN_CopyTextureRegion = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,
    const D3D12_TEXTURE_COPY_LOCATION*, UINT, UINT, UINT,
    const D3D12_TEXTURE_COPY_LOCATION*, const D3D12_BOX*);

using PFN_CopyBufferRegion = void (STDMETHODCALLTYPE*)
    (ID3D12GraphicsCommandList*, ID3D12Resource*, UINT64, ID3D12Resource*,
     UINT64, UINT64);
// vrcam post-DLSS crop fix: command-list viewport/scissor + reset hooks (slots 21/22/10).
using PFN_RSSetViewports = void (STDMETHODCALLTYPE*)
    (ID3D12GraphicsCommandList*, UINT, const D3D12_VIEWPORT*);
using PFN_RSSetScissorRects = void (STDMETHODCALLTYPE*)
    (ID3D12GraphicsCommandList*, UINT, const D3D12_RECT*);
using PFN_GfxReset = HRESULT (STDMETHODCALLTYPE*)
    (ID3D12GraphicsCommandList*, ID3D12CommandAllocator*, ID3D12PipelineState*);
using CreateRTVFn = void (STDMETHODCALLTYPE*)(ID3D12Device*, ID3D12Resource*,
    const D3D12_RENDER_TARGET_VIEW_DESC*, D3D12_CPU_DESCRIPTOR_HANDLE);
PFN_CreateCommandList    g_orig_CreateCommandList = nullptr;
PFN_ResourceBarrier      g_orig_ResourceBarrier = nullptr;
PFN_CopyResource         g_orig_CopyResource = nullptr;
CreateRTVFn g_orig_CreateRTV = nullptr;
// Defined further down, with the constant-block probe it serves; declared here because the
// device vtable is patched long before that point.
using CreateCBVFn = void (STDMETHODCALLTYPE*)(ID3D12Device*,
    const D3D12_CONSTANT_BUFFER_VIEW_DESC*, D3D12_CPU_DESCRIPTOR_HANDLE);
CreateCBVFn g_orig_CreateCBV = nullptr;
// hk_CreateCBV is declared in Stereo/StereoInternal.hpp; its definition moved to another file.
// Exposure probe: defined with the rest of it further down, used by the barrier hook above it.
// expo_probe_copy moved with the census; declared in Stereo/StereoInternal.hpp.
// expo_mirror moved with the census; declared in Stereo/StereoInternal.hpp.
// expo_probe_report moved with the census; declared in Stereo/StereoInternal.hpp.
// tile_is_grid moved with the census; declared in Stereo/StereoInternal.hpp.
// cull_count_note moved with the census; declared in Stereo/StereoInternal.hpp.
// cull_count_report moved with the census; declared in Stereo/StereoInternal.hpp.
// tile_probe_copy moved with the census; declared in Stereo/StereoInternal.hpp.
// tile_probe_report moved with the census; declared in Stereo/StereoInternal.hpp.
static std::atomic<bool> g_rtv_hook_installed{false};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorGameQueue = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorReadyFence = 0;

std::mutex g_game_object_mtx;
ID3D12Device* g_game_device = nullptr;

// The D3D12 debug-layer message drain that lived here is DELETED, its question answered on
// 2026-08-29. Forced on from outside for this executable alone, the layer reports two errors
// continuously -- 538, a DiscardResource on a resource the runtime tracks as COMMON where
// UNORDERED_ACCESS is required, and 1314, a ClearUnorderedAccessViewUint whose descriptor is
// not from the bound heap. Both were measured over a minute of gameplay with the port's render
// hooks installed and again with them out entirely (bin/x64/vrport_nostereo.txt):
//
//     538    5.0/s with hooks   4.0/s without
//     1314  18.8/s with hooks  20.0/s without
//
// Identical. They are the engine's own, they do not crash a vanilla install, and the port
// neither causes nor multiplies them. Nothing here is a lead.
//
// Worth recording for whoever forces the layer on next: the layer costs a large part of the
// frame rate, and while it was on the 0x88 driver fault did not occur once in 1457 seconds
// against a median of about two minutes without it -- suggestive, and the reason the control
// run for that fault has to be done with the layer OFF.
ID3D12CommandQueue* g_game_queue = nullptr;
// Accessors for the game-view ImGui overlay (overlay_imgui.cpp).
extern "C" ID3D12Device*       CyberpunkVR_GetGameDevice() { return g_game_device; }
extern "C" ID3D12CommandQueue* CyberpunkVR_GetGameQueue()  { return g_game_queue; }

// Moved to src/Stereo/Mirror.cpp: the game-side copy objects, borrowed by the command-list hooks.

// hk_CreateRTV is declared in Stereo/StereoInternal.hpp; its definition moved to another file.
// hk_ResourceBarrier is declared in Stereo/StereoInternal.hpp; its definition moved to another file.
// hk_OMSetRenderTargets is declared in Stereo/StereoInternal.hpp; its definition moved to another file.
// hk_CopyBufferRegion moved with the census; declared in Stereo/StereoInternal.hpp.
// hk_RSSetViewports moved with the DLSS band to src/Stereo/Dlss.cpp; declared in
// Stereo/StereoInternal.hpp.
// hk_RSSetScissorRects moved with the DLSS band; declared in Stereo/StereoInternal.hpp.
// hk_GfxReset moved with the DLSS band; declared in Stereo/StereoInternal.hpp.
// POST-DLSS CROP FIX (D3D12-level, definitive): per Nsight, the crop is a vrcam-only post-DLSS
// fullscreen blit/tonemap (PipelineState_563) that READS the 2444 DLSS output and WRITES a 2444
// RT but with a render-res (1418) viewport+scissor -> only the top-left 1418 is filled = crop.
// MAIN has no such graphics pass (it composites via compute). The blit's viewport is NOT the
// view render-res field we can flip (VP+0x34 feeds pre-DLSS passes too), so we correct it at the
// D3D12 layer: on the SAME command-list/thread that records the vrcam DLSS eval, any viewport or
// scissor set to the vrcam RENDER size (1418) AFTER the eval is upscaled to the vrcam OUTPUT size
// (2444). Thread-local phase (set at vrcam eval, cleared at the command-list Reset that begins the
// next frame's recording) keeps this from touching pre-DLSS passes; the render-size match keeps it
// off MAIN (main never uses the square vrcam render size). Dims come from g_vrcam_dlss_r*/o*.
thread_local bool t_vrcam_dlss_post = false;   // (legacy, unused) vrcam post-DLSS phase marker
// THREAD-AGNOSTIC crop fix: OMSetRenderTargets and RSSetViewports/ScissorRects for a given draw
// run consecutively on the SAME command-list recording thread. So we capture the primary bound
// RT's dimensions per-thread here, and the viewport/scissor hooks upscale a viewport that
// under-fills that RT. This does not depend on which thread the DLSS eval ran on (the eval and
// the vrcam blit are NOT reliably co-threaded in the live frame graph).
thread_local UINT t_cur_rt_w = 0;   // primary bound RT width  on THIS thread
thread_local UINT t_cur_rt_h = 0;   // primary bound RT height on THIS thread
// Set while the vrcam CopyToTexture (sub_140377B58) node is recording on THIS thread: the crop
// pass whose viewport comes from a render-res SOURCE resource (not VP+0x34), so the VP-swap alone
// can't move it. While set, hk_RSSetViewports/Scissor upscale the render-res (1418) viewport to
// output (2444) -> the copy fills the whole 2444 target. Scoped to just this one node + vrcam.
thread_local bool t_copytotex = false;
// (removed: investigation-only viewport/blit diagnostics -- crop root FOUND = raster tonemap
//  sub_140768510 gated by group 20; fixed natively in Detour_RenderRes via view+0x17D0 match-main.
//  The stack-capture + band-aid are no longer needed, and the per-viewport RtlCaptureStackBackTrace
//  was pure overhead.)
// Broad RTV -> dims map (ANY format/size, unlike the RGBA8-only mirror candidate list) so the
// OMSetRenderTargets hook can tell the size of the bound RT for the R11G11B10 DLSS-output blit.
// `res` so a bound RTV can be resolved back to its texture -- needed to name the surface
// RenderVisionElements draws the scanner's object outline into (see the vision snapshot).
// EVERY RTV THE GAME CREATES, BY DESCRIPTOR HANDLE. Read on the recording threads to answer "what
// is this bind pointing at", which is how the HUD surface is identified and how the vrcam output
// is recognised -- so a lookup that comes back empty does not degrade anything, it switches a
// feature off.
//
// It used to be 2048 entries with `if (n >= size) return;` -- silently stop accepting, forever.
// Past that point every newly created descriptor was invisible, and a frame-graph rebuild creates
// a whole new set: the HUD node kept binding its surface and we could no longer say what the
// handle meant, so `[hud] surface named by DrawHUD` stopped appearing and the second eye lost the
// HUD for the session. The same saturation is why a [rtvpick] miss reported "descriptor never seen
// created" for a target the engine had plainly just created.
//
// A ring now, and it says so when it first wraps. Overwriting the OLDEST entry is the right trade
// here: a descriptor that has not been re-created in eight thousand creations is one the engine has
// almost certainly recycled anyway, and the loop below already refreshes an entry in place when its
// handle comes round again.
//
// `handle` is atomic so publication is ordered rather than hoped for: the writer clears it, fills
// the rest, then stores the handle last; a reader that sees the handle therefore sees the fields
// that go with it. Readers do not take the mutex -- this is consulted on every OMSetRenderTargets.
// struct RtvDimEntry now lives in Stereo/StereoInternal.hpp: the HUD reads a bound target's
// dimensions out of the array below.
std::array<RtvDimEntry, 8192> g_rtv_dim_map{};
std::atomic<uint32_t> g_rtv_dim_count{0};
std::atomic<uint32_t> g_rtv_dim_next{0};
bool g_rtv_dim_wrapped_logged = false;
// How often the HUD node binds a target, and how often we cannot say what that bind points at.
// The second number rising with the first is the map above failing to answer, which is the whole
// difference between "the HUD moved" and "we went blind to it".
std::atomic<uint64_t> g_hud_node_binds{0};
std::atomic<uint64_t> g_hud_node_unresolved{0};
std::mutex g_rtv_dim_mtx;
// d12_present_thread and d12_submit_mirror_copy moved to src/Stereo/Mirror.cpp; both are declared
// in Stereo/StereoInternal.hpp. A `static` forward declaration here would promise a definition in
// THIS file -- the shape that has now appeared in six extractions.

static void STDMETHODCALLTYPE Hook_ExecuteCommandLists(
        ID3D12CommandQueue* self, UINT n, ID3D12CommandList* const* lists) {
    CyberpunkVR_DebugExecTotal = g_exec_total.fetch_add(n, std::memory_order_relaxed) + n;
    // Do the single 11on12 copy+present only once the game actually SUBMITS the
    // command list that wrote the vrcam dtex (the one that recorded the blit's
    // RENDER_TARGET->read barrier). Queue ordering then guarantees our copy reads
    // the freshly written frame. Non-blocking (Flush + Present(0)) => no FPS drop.
    ID3D12GraphicsCommandList* pending =
        g_mirror_pending_list.load(std::memory_order_acquire);
    bool submits_blit = false;
    if (pending && lists) {
        for (UINT i = 0; i < n; ++i) {
            if (lists[i] == reinterpret_cast<ID3D12CommandList*>(pending)) {
                submits_blit = true; break;
            }
        }
    }
    g_orig_ExecuteCommandLists(self, n, lists);
    // The game just submitted the list that wrote the vrcam final. That target rests
    // permanently in RENDER_TARGET (never read back -> no RT->read barrier exists),
    // so we copy it out ourselves with one tiny submit on the game queue right here,
    // from its known fixed state. Queue order guarantees we read the fresh frame.
    if (submits_blit && self == g_game_queue) {
        g_mirror_pending_list.store(nullptr, std::memory_order_release);
        d12_submit_mirror_copy(self);
    }
}

static void patch_queue_vtable(void* queue) {
    if (!queue) return;
    bool e = false;
    if (!g_queue_vtable_patched.compare_exchange_strong(e, true)) return;
    void** vt = *reinterpret_cast<void***>(queue);
    DWORD oldp = 0;
    if (VirtualProtect(&vt[10], sizeof(void*), PAGE_READWRITE, &oldp)) {
        g_orig_ExecuteCommandLists = reinterpret_cast<PFN_ExecuteCommandLists>(vt[10]);
        vt[10] = reinterpret_cast<void*>(&Hook_ExecuteCommandLists);
        DWORD junk = 0;
        VirtualProtect(&vt[10], sizeof(void*), oldp, &junk);
        log("[exec] ExecuteCommandLists hooked (queue=%p orig=%p)",
            queue, (void*)g_orig_ExecuteCommandLists);
    } else {
        g_queue_vtable_patched.store(false, std::memory_order_release);
    }
}

static HRESULT STDMETHODCALLTYPE Hook_CreateCommandQueue(
        ID3D12Device* self, const D3D12_COMMAND_QUEUE_DESC* desc, REFIID riid, void** out) {
    HRESULT hr = g_orig_CreateCommandQueue(self, desc, riid, out);
    if (SUCCEEDED(hr) && out && *out) {
        auto* queue = reinterpret_cast<ID3D12CommandQueue*>(*out);
        if (desc && desc->Type == D3D12_COMMAND_LIST_TYPE_DIRECT) {
            std::lock_guard<std::mutex> lock(g_game_object_mtx);
            if (!g_game_queue) {
                queue->AddRef();
                g_game_queue = queue;
                CyberpunkVR_DebugMirrorGameQueue = reinterpret_cast<uint64_t>(queue);
                log("[mirror] captured game DIRECT queue=%p device=%p", queue, self);
            }
        }
        patch_queue_vtable(queue);
    }
    return hr;
}

// Filled by the PSO-creation hooks below; consumed by the sight probe further down.
// pso_ids_record moved with the census; declared in Stereo/StereoInternal.hpp.
// hud_adopt_by_node moved with the HUD; declared in Stereo/StereoInternal.hpp.
std::atomic<uint64_t> g_hud_snap_tick{0};
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_HudNodeRva;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_HudNodeRva2;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_HudByNode;
// fnv1a moved with the census; declared in Stereo/StereoInternal.hpp.
// sight_ps_dump moved with the census; declared in Stereo/StereoInternal.hpp.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_SightPsHash;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_SightPsDump;

static std::array<CommandListVtableHook, 16> g_command_list_vtable_hooks{};
static std::atomic<uint32_t> g_command_list_vtable_hook_count{0};
static std::mutex g_command_list_vtable_hook_mtx;

const CommandListVtableHook* command_list_hook_entry(
        ID3D12GraphicsCommandList* command_list) {
    if (!command_list) return nullptr;
    void** vtable = *reinterpret_cast<void***>(command_list);
    const uint32_t count =
        g_command_list_vtable_hook_count.load(std::memory_order_acquire);
    for (uint32_t i = 0; i < count; ++i) {
        if (g_command_list_vtable_hooks[i].vtable == vtable)
            return &g_command_list_vtable_hooks[i];
    }
    return nullptr;
}
PFN_OMSetRenderTargets command_list_original_om(
        ID3D12GraphicsCommandList* command_list) {
    const CommandListVtableHook* e = command_list_hook_entry(command_list);
    return e ? e->original : nullptr;
}

static void patch_command_list_vtable(void* command_list) {
    if (!command_list) return;
    void** vtable = *reinterpret_cast<void***>(command_list);
    std::lock_guard<std::mutex> lock(g_command_list_vtable_hook_mtx);
    uint32_t count =
        g_command_list_vtable_hook_count.load(std::memory_order_relaxed);
    for (uint32_t i = 0; i < count; ++i) {
        if (g_command_list_vtable_hooks[i].vtable == vtable) return;
    }
    if (count >= g_command_list_vtable_hooks.size()) return;
    PFN_OMSetRenderTargets om_orig = nullptr;
    PFN_ResourceBarrier    rb_orig = nullptr;
    DWORD oldp = 0;
    if (VirtualProtect(&vtable[46], sizeof(void*), PAGE_READWRITE, &oldp)) {
        om_orig = reinterpret_cast<PFN_OMSetRenderTargets>(vtable[46]);
        vtable[46] = reinterpret_cast<void*>(&hk_OMSetRenderTargets);
        DWORD junk = 0; VirtualProtect(&vtable[46], sizeof(void*), oldp, &junk);
    } else {
        return;
    }
    DWORD oldp2 = 0;
    if (VirtualProtect(&vtable[26], sizeof(void*), PAGE_READWRITE, &oldp2)) {
        rb_orig = reinterpret_cast<PFN_ResourceBarrier>(vtable[26]);
        vtable[26] = reinterpret_cast<void*>(&hk_ResourceBarrier);
        DWORD junk = 0; VirtualProtect(&vtable[26], sizeof(void*), oldp2, &junk);
    }
    PFN_CopyBufferRegion cbr_orig = nullptr;
    DWORD oldp3 = 0;
    if (VirtualProtect(&vtable[15], sizeof(void*), PAGE_READWRITE, &oldp3)) {
        cbr_orig = reinterpret_cast<PFN_CopyBufferRegion>(vtable[15]);
        vtable[15] = reinterpret_cast<void*>(&hk_CopyBufferRegion);
        DWORD junk = 0; VirtualProtect(&vtable[15], sizeof(void*), oldp3, &junk);
    }
    PFN_ExecuteIndirect ind_orig = nullptr;
    DWORD oldpI = 0;
    if (VirtualProtect(&vtable[59], sizeof(void*), PAGE_READWRITE, &oldpI)) {
        ind_orig = reinterpret_cast<PFN_ExecuteIndirect>(vtable[59]);
        vtable[59] = reinterpret_cast<void*>(&hk_ExecuteIndirect);
        DWORD junk = 0; VirtualProtect(&vtable[59], sizeof(void*), oldpI, &junk);
    }
    PFN_Dispatch disp_orig = nullptr;
    DWORD oldpD = 0;
    if (VirtualProtect(&vtable[14], sizeof(void*), PAGE_READWRITE, &oldpD)) {
        disp_orig = reinterpret_cast<PFN_Dispatch>(vtable[14]);
        vtable[14] = reinterpret_cast<void*>(&hk_Dispatch);
        DWORD junk = 0; VirtualProtect(&vtable[14], sizeof(void*), oldpD, &junk);
    }
    // POST-DLSS CROP FIX: RSSetViewports(21), RSSetScissorRects(22), Reset(10).
    PFN_RSSetViewports    vp_orig  = nullptr;
    PFN_RSSetScissorRects sc_orig  = nullptr;
    PFN_GfxReset          rst_orig = nullptr;
    DWORD oldp4 = 0;
    if (VirtualProtect(&vtable[21], sizeof(void*), PAGE_READWRITE, &oldp4)) {
        vp_orig = reinterpret_cast<PFN_RSSetViewports>(vtable[21]);
        vtable[21] = reinterpret_cast<void*>(&hk_RSSetViewports);
        DWORD junk = 0; VirtualProtect(&vtable[21], sizeof(void*), oldp4, &junk);
    }
    DWORD oldp5 = 0;
    if (VirtualProtect(&vtable[22], sizeof(void*), PAGE_READWRITE, &oldp5)) {
        sc_orig = reinterpret_cast<PFN_RSSetScissorRects>(vtable[22]);
        vtable[22] = reinterpret_cast<void*>(&hk_RSSetScissorRects);
        DWORD junk = 0; VirtualProtect(&vtable[22], sizeof(void*), oldp5, &junk);
    }
    DWORD oldp6 = 0;
    if (VirtualProtect(&vtable[10], sizeof(void*), PAGE_READWRITE, &oldp6)) {
        rst_orig = reinterpret_cast<PFN_GfxReset>(vtable[10]);
        vtable[10] = reinterpret_cast<void*>(&hk_GfxReset);
        DWORD junk = 0; VirtualProtect(&vtable[10], sizeof(void*), oldp6, &junk);
    }
    auto cr = reinterpret_cast<PFN_CopyResource>(vtable[17]);      // raw, for appending
    PFN_DrawInstanced dr_orig = nullptr;
    DWORD oldpDr = 0;
    if (VirtualProtect(&vtable[12], sizeof(void*), PAGE_READWRITE, &oldpDr)) {
        dr_orig = reinterpret_cast<PFN_DrawInstanced>(vtable[12]);
        vtable[12] = reinterpret_cast<void*>(&hk_DrawInstanced);
        DWORD junk = 0; VirtualProtect(&vtable[12], sizeof(void*), oldpDr, &junk);
    }
    PFN_DrawIndexedInstanced dri_orig = nullptr;
    DWORD oldpDi = 0;
    if (VirtualProtect(&vtable[13], sizeof(void*), PAGE_READWRITE, &oldpDi)) {
        dri_orig = reinterpret_cast<PFN_DrawIndexedInstanced>(vtable[13]);
        vtable[13] = reinterpret_cast<void*>(&hk_DrawIndexedInstanced);
        DWORD junk = 0; VirtualProtect(&vtable[13], sizeof(void*), oldpDi, &junk);
    }
    PFN_IASetVertexBuffers iavb_orig = nullptr;
    DWORD oldpVb = 0;
    if (VirtualProtect(&vtable[44], sizeof(void*), PAGE_READWRITE, &oldpVb)) {
        iavb_orig = reinterpret_cast<PFN_IASetVertexBuffers>(vtable[44]);
        vtable[44] = reinterpret_cast<void*>(&hk_IASetVertexBuffers);
        DWORD junk = 0; VirtualProtect(&vtable[44], sizeof(void*), oldpVb, &junk);
    }
    PFN_SetPipelineState sps_orig = nullptr;
    DWORD oldpSp = 0;
    if (VirtualProtect(&vtable[25], sizeof(void*), PAGE_READWRITE, &oldpSp)) {
        sps_orig = reinterpret_cast<PFN_SetPipelineState>(vtable[25]);
        vtable[25] = reinterpret_cast<void*>(&hk_SetPipelineState);
        DWORD junk = 0; VirtualProtect(&vtable[25], sizeof(void*), oldpSp, &junk);
    }
    // Slot 47, ClearDepthStencilView. Hooked so ONE clear can be withheld -- MAIN's clear of the shadow
    // atlas, which the second view has already filled with identical content this frame. Everything else
    // about that pass, including the NON_PIXEL_SHADER_RESOURCE -> DEPTH_WRITE transition the capture shows
    // it making, still happens: skipping the whole node instead removes that transition and the atlas is
    // then written and sampled in a state the graph does not expect, which is what made shadows flicker.
    PFN_ClearDepthStencilView cds_orig = nullptr;
    DWORD oldpCd = 0;
    if (VirtualProtect(&vtable[47], sizeof(void*), PAGE_READWRITE, &oldpCd)) {
        cds_orig = reinterpret_cast<PFN_ClearDepthStencilView>(vtable[47]);
        vtable[47] = reinterpret_cast<void*>(&hk_ClearDepthStencilView);
        DWORD junk = 0; VirtualProtect(&vtable[47], sizeof(void*), oldpCd, &junk);
    }
    auto ct = reinterpret_cast<PFN_CopyTextureRegion>(vtable[16]); // raw, tile-grid probe
    g_command_list_vtable_hooks[count] =
        {vtable, om_orig, rb_orig, rb_orig, cr, ct, ind_orig, dr_orig, dri_orig, cbr_orig,
         disp_orig, vp_orig, sc_orig, rst_orig, sps_orig, iavb_orig, cds_orig};
    g_command_list_vtable_hook_count.store(count + 1, std::memory_order_release);
    log("[mirror] command-list hooked list=%p vt=%p om=%p rb=%p cr=%p",
        command_list, vtable, (void*)om_orig, (void*)rb_orig, (void*)cr);
}

// ID3D12Device slots 27 and 29. Only to answer "which resource owns this GPU address" -- a
// vertex-buffer view carries an address and nothing else, and D3D12 offers no way back.
using PFN_CreateCommittedResource = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, const D3D12_HEAP_PROPERTIES*, D3D12_HEAP_FLAGS,
    const D3D12_RESOURCE_DESC*, D3D12_RESOURCE_STATES, const D3D12_CLEAR_VALUE*, REFIID, void**);
using PFN_CreatePlacedResource = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, ID3D12Heap*, UINT64, const D3D12_RESOURCE_DESC*,
    D3D12_RESOURCE_STATES, const D3D12_CLEAR_VALUE*, REFIID, void**);
static PFN_CreateCommittedResource g_orig_CreateCommitted = nullptr;
static PFN_CreatePlacedResource    g_orig_CreatePlaced = nullptr;
// buf_note moved with the census; declared in Stereo/StereoInternal.hpp.

static void buf_note_created(void* out, const D3D12_RESOURCE_DESC* d) {
    if (!out || !d || d->Dimension != D3D12_RESOURCE_DIMENSION_BUFFER) return;
    if (d->Width < 4096) return;                    // instance/vertex streams, not tiny scratch
    auto* res = static_cast<ID3D12Resource*>(out);
    uint64_t va = 0;
    __try { va = res->GetGPUVirtualAddress(); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    buf_note(res, va, d->Width);
}

static HRESULT STDMETHODCALLTYPE Hook_CreateCommittedResource(
        ID3D12Device* self, const D3D12_HEAP_PROPERTIES* hp, D3D12_HEAP_FLAGS hf,
        const D3D12_RESOURCE_DESC* d, D3D12_RESOURCE_STATES st,
        const D3D12_CLEAR_VALUE* cv, REFIID riid, void** out) {
    HRESULT hr = g_orig_CreateCommitted(self, hp, hf, d, st, cv, riid, out);
    if (SUCCEEDED(hr) && out && *out) buf_note_created(*out, d);
    return hr;
}

static HRESULT STDMETHODCALLTYPE Hook_CreatePlacedResource(
        ID3D12Device* self, ID3D12Heap* heap, UINT64 off, const D3D12_RESOURCE_DESC* d,
        D3D12_RESOURCE_STATES st, const D3D12_CLEAR_VALUE* cv, REFIID riid, void** out) {
    HRESULT hr = g_orig_CreatePlaced(self, heap, off, d, st, cv, riid, out);
    if (SUCCEEDED(hr) && out && *out) buf_note_created(*out, d);
    return hr;
}

// ID3D12Device slot 16. There is no reflection from an ID3D12RootSignature back to its
// description, so the only way to learn the binding contract our replacement shaders must live
// inside is to keep the blob at creation. What we are looking for is a root parameter this
// material does NOT use -- a spare root CBV or a set of root constants -- because occupying one
// of those needs no signature change at all: the PSO keeps the game's signature, nothing is
// rebound, and the muzzle direction can simply be set before the draw.
using PFN_CreateRootSignature = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, UINT, const void*, SIZE_T, REFIID, void**);
static PFN_CreateRootSignature g_orig_CreateRootSig = nullptr;
static std::unordered_map<void*, std::vector<uint8_t>> g_rootsig_blobs;
static std::mutex g_rootsig_mtx;

static HRESULT STDMETHODCALLTYPE Hook_CreateRootSignature(
        ID3D12Device* self, UINT nodeMask, const void* blob, SIZE_T len,
        REFIID riid, void** out) {
    HRESULT hr = g_orig_CreateRootSig(self, nodeMask, blob, len, riid, out);
    if (SUCCEEDED(hr) && out && *out && blob && len && len < (1u << 20)) {
        std::lock_guard<std::mutex> lk(g_rootsig_mtx);
        if (g_rootsig_blobs.size() < 4096)
            g_rootsig_blobs[*out].assign(static_cast<const uint8_t*>(blob),
                                         static_cast<const uint8_t*>(blob) + len);
    }
    return hr;
}

// Print the layout once, for the signature the sight's pipeline uses.
static void rootsig_dump(void* rs) {
    if (!rs) return;
    static std::atomic<void*> s_done{nullptr};
    void* expected = nullptr;
    if (!s_done.compare_exchange_strong(expected, rs)) return;
    std::vector<uint8_t> blob;
    {
        std::lock_guard<std::mutex> lk(g_rootsig_mtx);
        auto it = g_rootsig_blobs.find(rs);
        if (it == g_rootsig_blobs.end()) {
            log("[rootsig] sight signature %p: blob not captured (created before the hook)", rs);
            return;
        }
        blob = it->second;
    }
    ID3D12VersionedRootSignatureDeserializer* des = nullptr;
    if (FAILED(D3D12CreateVersionedRootSignatureDeserializer(
            blob.data(), blob.size(), IID_PPV_ARGS(&des))) || !des) {
        log("[rootsig] sight signature %p: deserialize failed (%zu bytes)", rs, blob.size());
        return;
    }
    const D3D12_VERSIONED_ROOT_SIGNATURE_DESC* v = nullptr;
    if (FAILED(des->GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1, &v)) || !v) {
        des->Release();
        log("[rootsig] sight signature %p: no 1.1 view", rs);
        return;
    }
    const auto& d = v->Desc_1_1;
    log("[rootsig] sight signature %p: %u params, %u samplers, flags=0x%X",
        rs, d.NumParameters, d.NumStaticSamplers, (unsigned)d.Flags);
    for (UINT i = 0; i < d.NumParameters; ++i) {
        const auto& p = d.pParameters[i];
        const unsigned vis = (unsigned)p.ShaderVisibility;
        switch (p.ParameterType) {
        case D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS:
            log("[rootsig]  [%2u] CONSTANTS b%u space%u x%u  vis=%u", i,
                p.Constants.ShaderRegister, p.Constants.RegisterSpace,
                p.Constants.Num32BitValues, vis);
            break;
        case D3D12_ROOT_PARAMETER_TYPE_CBV:
            log("[rootsig]  [%2u] CBV       b%u space%u  vis=%u", i,
                p.Descriptor.ShaderRegister, p.Descriptor.RegisterSpace, vis);
            break;
        case D3D12_ROOT_PARAMETER_TYPE_SRV:
            log("[rootsig]  [%2u] SRV       t%u space%u  vis=%u", i,
                p.Descriptor.ShaderRegister, p.Descriptor.RegisterSpace, vis);
            break;
        case D3D12_ROOT_PARAMETER_TYPE_UAV:
            log("[rootsig]  [%2u] UAV       u%u space%u  vis=%u", i,
                p.Descriptor.ShaderRegister, p.Descriptor.RegisterSpace, vis);
            break;
        case D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE: {
            char r[400]; int u = 0; r[0] = 0;
            for (UINT k = 0; k < p.DescriptorTable.NumDescriptorRanges && u < 340; ++k) {
                const auto& rг = p.DescriptorTable.pDescriptorRanges[k];
                const char* t = rг.RangeType == D3D12_DESCRIPTOR_RANGE_TYPE_CBV ? "b"
                              : rг.RangeType == D3D12_DESCRIPTOR_RANGE_TYPE_SRV ? "t"
                              : rг.RangeType == D3D12_DESCRIPTOR_RANGE_TYPE_UAV ? "u" : "s";
                u += snprintf(r + u, sizeof(r) - u, "%s%u+%u/sp%u ", t,
                              rг.BaseShaderRegister, rг.NumDescriptors, rг.RegisterSpace);
            }
            log("[rootsig]  [%2u] TABLE     vis=%u  %s", i, vis, r);
            break;
        }
        default: log("[rootsig]  [%2u] type=%u", i, (unsigned)p.ParameterType); break;
        }
    }
    des->Release();
}

// ID3D12Device slot 10 and ID3D12Device2 slot 47. Both are entry points the engine may use, and
// which one it takes is not worth guessing -- hooking both costs one pointer each. All they do
// here is remember (pso -> shader hashes); substitution, when it comes, happens in the same place.
using PFN_CreateGraphicsPipelineState = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, const D3D12_GRAPHICS_PIPELINE_STATE_DESC*, REFIID, void**);
using PFN_CreatePipelineState = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, const D3D12_PIPELINE_STATE_STREAM_DESC*, REFIID, void**);
static PFN_CreateGraphicsPipelineState g_orig_CreateGfxPso = nullptr;
static PFN_CreatePipelineState         g_orig_CreatePso = nullptr;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPsoGfx = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPsoStream = 0;

// The replacement is loaded from a FILE rather than baked into the DLL. Iterating on it then
// costs a dxc run and a game restart instead of a full rebuild, and the shader stays readable
// next to the binary it patches.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_SightPsSwap = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugSightSwaps = 0;
// Measured: this pixel shader is used by TWO pipelines and only one of them draws the reticle.
// Keying the swap on the PAIR (pixel AND vertex hash) picks that one exactly, instead of relying
// on the other pipeline failing to build.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_SightVsHash = 0x9228439BF72D91DBull;
static std::vector<uint8_t> g_sight_blob;      // pixel
static std::vector<uint8_t> g_sight_vs_blob;   // vertex
static std::atomic<int> g_sight_blob_state{0};   // 0 untried, 1 loaded, -1 missing

static bool sight_blob_ready() {
    int st = g_sight_blob_state.load(std::memory_order_acquire);
    if (st) return st > 0;
    static std::mutex mtx;
    std::lock_guard<std::mutex> lk(mtx);
    st = g_sight_blob_state.load(std::memory_order_relaxed);
    if (st) return st > 0;
    char path[MAX_PATH]{};
    HMODULE self = nullptr;
    if (GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                           GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                           reinterpret_cast<LPCSTR>(&sight_blob_ready), &self) && self &&
        GetModuleFileNameA(self, path, MAX_PATH)) {
        char* slash = strrchr(path, '\\');
        if (slash) {
            *(slash + 1) = 0;
            const size_t dirLen = strlen(path);
            bool bothOk = true;
            for (int which = 0; which < 2 && bothOk; ++which) {
                path[dirLen] = 0;
                strcat_s(path, which ? "CyberpunkVR_SightVs.dxil" : "CyberpunkVR_SightPs.dxil");
                std::vector<uint8_t>& dstBlob = which ? g_sight_vs_blob : g_sight_blob;
                dstBlob.clear();
                HANDLE f = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ, nullptr,
                                       OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
                if (f != INVALID_HANDLE_VALUE) {
                    LARGE_INTEGER sz{};
                    if (GetFileSizeEx(f, &sz) && sz.QuadPart > 64 && sz.QuadPart < (4 << 20)) {
                        dstBlob.resize(static_cast<size_t>(sz.QuadPart));
                        DWORD got = 0;
                        if (!(ReadFile(f, dstBlob.data(), static_cast<DWORD>(sz.QuadPart),
                                       &got, nullptr) && got == sz.QuadPart &&
                              memcmp(dstBlob.data(), "DXBC", 4) == 0)) {
                            dstBlob.clear();
                        }
                    }
                    CloseHandle(f);
                }
                if (dstBlob.empty()) {
                    log("[pso] sight replacement NOT found (%s) -- original shaders kept", path);
                    bothOk = false;
                }
            }
            // Both or neither: the pixel shader reads the glass size the vertex shader writes,
            // so half a swap would be worse than none.
            if (bothOk) {
                g_sight_blob_state.store(1, std::memory_order_release);
                log("[pso] sight replacement loaded: PS %zu B, VS %zu B",
                    g_sight_blob.size(), g_sight_vs_blob.size());
                return true;
            }
            g_sight_blob.clear();
            g_sight_vs_blob.clear();
        }
    }
    g_sight_blob_state.store(-1, std::memory_order_release);
    return false;
}

static bool sight_is_target(const D3D12_SHADER_BYTECODE& ps, const D3D12_SHADER_BYTECODE& vs) {
    return CyberpunkVR_SightPsSwap && ps.pShaderBytecode && ps.BytecodeLength &&
           vs.pShaderBytecode && vs.BytecodeLength &&
           fnv1a(ps.pShaderBytecode, ps.BytecodeLength) == CyberpunkVR_SightPsHash &&
           fnv1a(vs.pShaderBytecode, vs.BytecodeLength) == CyberpunkVR_SightVsHash &&
           sight_blob_ready();
}

static HRESULT STDMETHODCALLTYPE Hook_CreateGraphicsPipelineState(
        ID3D12Device* self, const D3D12_GRAPHICS_PIPELINE_STATE_DESC* desc,
        REFIID riid, void** out) {
    if (desc && desc->PS.pShaderBytecode && desc->PS.BytecodeLength &&
        fnv1a(desc->PS.pShaderBytecode, desc->PS.BytecodeLength) == CyberpunkVR_SightPsHash) {
        rootsig_dump(desc->pRootSignature);
    }
    if (desc && sight_is_target(desc->PS, desc->VS)) {
        D3D12_GRAPHICS_PIPELINE_STATE_DESC d = *desc;
        d.PS.pShaderBytecode = g_sight_blob.data();
        d.PS.BytecodeLength = g_sight_blob.size();
        d.VS.pShaderBytecode = g_sight_vs_blob.data();
        d.VS.BytecodeLength = g_sight_vs_blob.size();
        HRESULT hr2 = g_orig_CreateGfxPso(self, &d, riid, out);
        if (SUCCEEDED(hr2)) {
            ++CyberpunkVR_DebugSightSwaps;
            log("[pso] sight PS substituted (graphics desc) pso=%p", out ? *out : nullptr);
            if (out && *out) pso_ids_record(*out, desc->PS, desc->VS);  // keep the ORIGINAL id
            ++CyberpunkVR_DebugPsoGfx;
            return hr2;
        }
        // A rejected replacement must not cost the game its shader: fall through to the original.
        log("[pso] sight PS substitution REFUSED hr=0x%08X -- keeping the original",
            static_cast<unsigned>(hr2));
    }
    HRESULT hr = g_orig_CreateGfxPso(self, desc, riid, out);
    if (SUCCEEDED(hr) && out && *out && desc) {
        pso_ids_record(*out, desc->PS, desc->VS);
        ++CyberpunkVR_DebugPsoGfx;
    }
    return hr;
}

// The stream form is a tagged blob: {alignas(void*) SUBOBJECT_TYPE, payload} repeated. Walking it
// is the only way to see the shaders, and the walk has to respect the pointer alignment between
// entries or it desynchronises and reads garbage.
// Offsets of the PS and VS payloads inside the tagged stream, SIZE_MAX when absent. Kept in its
// own function on purpose: it needs SEH, and SEH cannot share a frame with objects that unwind.
static void pso_stream_find(const uint8_t* p, size_t len, size_t* psoff, size_t* vsoff) {
    *psoff = SIZE_MAX; *vsoff = SIZE_MAX;
    if (!p || !len) return;
    const uint8_t* base = p;
    const uint8_t* end = p + len;
    __try {
        while (p + sizeof(void*) <= end) {
            const auto type = *reinterpret_cast<const D3D12_PIPELINE_STATE_SUBOBJECT_TYPE*>(p);
            const uint8_t* payload = p + sizeof(void*);
            size_t plen = 0;
            switch (type) {
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS:
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS:
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DS:
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_HS:
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_GS:
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS:
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS:
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS:
                plen = sizeof(D3D12_SHADER_BYTECODE);
                if (payload + plen > end) { p = end; break; }
                if (type == D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS)
                    *psoff = static_cast<size_t>(payload - base);
                else if (type == D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS)
                    *vsoff = static_cast<size_t>(payload - base);
                break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE:
                plen = sizeof(void*); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_STREAM_OUTPUT:
                plen = sizeof(D3D12_STREAM_OUTPUT_DESC); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND:
                plen = sizeof(D3D12_BLEND_DESC); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_MASK:
                plen = sizeof(UINT); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER:
                plen = sizeof(D3D12_RASTERIZER_DESC); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL:
                plen = sizeof(D3D12_DEPTH_STENCIL_DESC); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT:
                plen = sizeof(D3D12_INPUT_LAYOUT_DESC); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_IB_STRIP_CUT_VALUE:
                plen = sizeof(D3D12_INDEX_BUFFER_STRIP_CUT_VALUE); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY:
                plen = sizeof(D3D12_PRIMITIVE_TOPOLOGY_TYPE); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS:
                plen = sizeof(D3D12_RT_FORMAT_ARRAY); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT:
                plen = sizeof(DXGI_FORMAT); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC:
                plen = sizeof(DXGI_SAMPLE_DESC); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_NODE_MASK:
                plen = sizeof(UINT); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CACHED_PSO:
                plen = sizeof(D3D12_CACHED_PIPELINE_STATE); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_FLAGS:
                plen = sizeof(D3D12_PIPELINE_STATE_FLAGS); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL1:
                plen = sizeof(D3D12_DEPTH_STENCIL_DESC1); break;
            case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING:
                plen = sizeof(D3D12_VIEW_INSTANCING_DESC); break;
            default:
                // Unknown tag: the walk can no longer be trusted, so stop rather than
                // resynchronise on a guess.
                p = end; plen = 0; break;
            }
            if (p >= end) break;
            p += (sizeof(void*) + plen + sizeof(void*) - 1) & ~(sizeof(void*) - 1);
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) { *psoff = SIZE_MAX; *vsoff = SIZE_MAX; }
}


static HRESULT STDMETHODCALLTYPE Hook_CreatePipelineState(
        ID3D12Device* self, const D3D12_PIPELINE_STATE_STREAM_DESC* desc,
        REFIID riid, void** out) {
    // Same substitution as the classic path, but the stream is the caller's memory, so it is
    // COPIED first and the copy is patched -- writing into the engine's own description would
    // outlive this call and be visible to whatever else reads it.
    if (desc && desc->pPipelineStateSubobjectStream && desc->SizeInBytes) {
        const uint8_t* src = static_cast<const uint8_t*>(desc->pPipelineStateSubobjectStream);
        size_t psoff = SIZE_MAX, vsoff = SIZE_MAX;
        pso_stream_find(src, desc->SizeInBytes, &psoff, &vsoff);
        if (psoff != SIZE_MAX && vsoff != SIZE_MAX) {
            const auto& bc = *reinterpret_cast<const D3D12_SHADER_BYTECODE*>(src + psoff);
            const auto& bcv = *reinterpret_cast<const D3D12_SHADER_BYTECODE*>(src + vsoff);
            if (sight_is_target(bc, bcv)) {
                std::vector<uint8_t> copy(src, src + desc->SizeInBytes);
                auto& nbc = *reinterpret_cast<D3D12_SHADER_BYTECODE*>(copy.data() + psoff);
                nbc.pShaderBytecode = g_sight_blob.data();
                nbc.BytecodeLength = g_sight_blob.size();
                auto& nbv = *reinterpret_cast<D3D12_SHADER_BYTECODE*>(copy.data() + vsoff);
                nbv.pShaderBytecode = g_sight_vs_blob.data();
                nbv.BytecodeLength = g_sight_vs_blob.size();
                D3D12_PIPELINE_STATE_STREAM_DESC nd = *desc;
                nd.pPipelineStateSubobjectStream = copy.data();
                HRESULT hr2 = g_orig_CreatePso(self, &nd, riid, out);
                if (SUCCEEDED(hr2)) {
                    ++CyberpunkVR_DebugSightSwaps;
                    ++CyberpunkVR_DebugPsoStream;
                    log("[pso] sight PS substituted (stream desc) pso=%p", out ? *out : nullptr);
                    return hr2;
                }
                log("[pso] sight PS substitution REFUSED hr=0x%08X (stream) -- original kept",
                    static_cast<unsigned>(hr2));
            }
        }
    }
    HRESULT hr = g_orig_CreatePso(self, desc, riid, out);
    if (SUCCEEDED(hr) && out && *out && desc && desc->pPipelineStateSubobjectStream) {
        size_t psoff = SIZE_MAX, vsoff = SIZE_MAX;
        pso_stream_find(static_cast<const uint8_t*>(desc->pPipelineStateSubobjectStream),
                        desc->SizeInBytes, &psoff, &vsoff);
        D3D12_SHADER_BYTECODE ps{}, vs{};
        const uint8_t* base = static_cast<const uint8_t*>(desc->pPipelineStateSubobjectStream);
        if (psoff != SIZE_MAX) ps = *reinterpret_cast<const D3D12_SHADER_BYTECODE*>(base + psoff);
        if (vsoff != SIZE_MAX) vs = *reinterpret_cast<const D3D12_SHADER_BYTECODE*>(base + vsoff);
        if (ps.pShaderBytecode || vs.pShaderBytecode) pso_ids_record(*out, ps, vs);
        ++CyberpunkVR_DebugPsoStream;
    }
    return hr;
}

static HRESULT STDMETHODCALLTYPE Hook_CreateCommandList(
        ID3D12Device* self, UINT node_mask, D3D12_COMMAND_LIST_TYPE type,
        ID3D12CommandAllocator* allocator, ID3D12PipelineState* initial_state,
        REFIID riid, void** out) {
    HRESULT hr = g_orig_CreateCommandList(
        self, node_mask, type, allocator, initial_state, riid, out);
    // COMPUTE as well as DIRECT. Registering only DIRECT lists left the engine's async-compute
    // list (`AsyncComputeDuringShadowmaps` in a capture) invisible to every hook here -- the
    // dispatch census, the ExecuteIndirect census, the barrier probes, all of them. That blind
    // spot is why the three 6x6x6 grading-volume builds each view does per frame never showed up
    // live even though both captures contain them.
    //
    // Safe to widen: command_list_hook_entry() keys on the VTABLE, not on the list pointer, so a
    // compute list simply finds the compute vtable's entry and the original is always called.
    // (A compute list has its own vtable, which is exactly why it was never patched before.)
    if (SUCCEEDED(hr) && out && *out &&
        (type == D3D12_COMMAND_LIST_TYPE_DIRECT || type == D3D12_COMMAND_LIST_TYPE_COMPUTE))
        patch_command_list_vtable(*out);
    return hr;
}

void patch_device_descriptor_slot(void* device) {
    if (!device) return;
    bool expected = false;
    if (!g_desc_vtable_patched.compare_exchange_strong(expected, true)) return;
    void** vt = *reinterpret_cast<void***>(device);
    // ID3D12Device slot 14 = CreateDescriptorHeap.
    DWORD oldp = 0;
    if (VirtualProtect(&vt[14], sizeof(void*), PAGE_READWRITE, &oldp)) {
        g_orig_CreateDescriptorHeap =
            reinterpret_cast<PFN_CreateDescriptorHeap>(vt[14]);
        vt[14] = reinterpret_cast<void*>(&Hook_CreateDescriptorHeap);
        DWORD junk = 0;
        VirtualProtect(&vt[14], sizeof(void*), oldp, &junk);
        log("[descheap] CreateDescriptorHeap hooked (dev=%p vt=%p orig=%p)",
            device, (void*)vt, (void*)g_orig_CreateDescriptorHeap);
    } else {
        g_desc_vtable_patched.store(false, std::memory_order_release);
        log("[descheap] FAILED to patch device vtable slot 14");
    }
    // ID3D12Device slot 8 = CreateCommandQueue (capture queues -> hook execute).
    if (g_enable_exec_probe) {
        DWORD o8 = 0;
        if (VirtualProtect(&vt[8], sizeof(void*), PAGE_READWRITE, &o8)) {
            g_orig_CreateCommandQueue = reinterpret_cast<PFN_CreateCommandQueue>(vt[8]);
            vt[8] = reinterpret_cast<void*>(&Hook_CreateCommandQueue);
            DWORD junk = 0;
            VirtualProtect(&vt[8], sizeof(void*), o8, &junk);
            log("[exec] CreateCommandQueue hooked (dev=%p)", device);
        }
    }
    // Slot 12 = CreateCommandList. Track each distinct D3D12Core command-list
    // vtable so OMSetRenderTargets can map the RTV bound specifically inside the
    // VRCAM CopyToTexture node. Slot 9 is CreateCommandAllocator (never patch it).
    DWORD o12 = 0;
    if (VirtualProtect(&vt[12], sizeof(void*), PAGE_READWRITE, &o12)) {
        g_orig_CreateCommandList = reinterpret_cast<PFN_CreateCommandList>(vt[12]);
        vt[12] = reinterpret_cast<void*>(&Hook_CreateCommandList);
        DWORD junk = 0;
        VirtualProtect(&vt[12], sizeof(void*), o12, &junk);
        log("[mirror] real device CreateCommandList hooked dev=%p orig=%p",
            device, (void*)g_orig_CreateCommandList);
    }
    // Slot 10 = CreateGraphicsPipelineState. Slot 47 = ID3D12Device2::CreatePipelineState, and
    // it is only touched when the device actually implements ID3D12Device2 -- patching a vtable
    // slot that may not exist is how a hook corrupts the object next to it.
    for (int k = 0; k < 2; ++k) {
        const UINT slot = k ? 29u : 27u;
        DWORD oldp = 0;
        if (!VirtualProtect(&vt[slot], sizeof(void*), PAGE_READWRITE, &oldp)) continue;
        if (k) {
            g_orig_CreatePlaced = reinterpret_cast<PFN_CreatePlacedResource>(vt[slot]);
            vt[slot] = reinterpret_cast<void*>(&Hook_CreatePlacedResource);
        } else {
            g_orig_CreateCommitted = reinterpret_cast<PFN_CreateCommittedResource>(vt[slot]);
            vt[slot] = reinterpret_cast<void*>(&Hook_CreateCommittedResource);
        }
        DWORD junk = 0; VirtualProtect(&vt[slot], sizeof(void*), oldp, &junk);
    }
    log("[sightaxis] buffer-address map hooked dev=%p", device);
    DWORD o16 = 0;
    if (VirtualProtect(&vt[16], sizeof(void*), PAGE_READWRITE, &o16)) {
        g_orig_CreateRootSig = reinterpret_cast<PFN_CreateRootSignature>(vt[16]);
        vt[16] = reinterpret_cast<void*>(&Hook_CreateRootSignature);
        DWORD junk = 0;
        VirtualProtect(&vt[16], sizeof(void*), o16, &junk);
        log("[rootsig] CreateRootSignature hooked dev=%p", device);
    }
    DWORD o10 = 0;
    if (VirtualProtect(&vt[10], sizeof(void*), PAGE_READWRITE, &o10)) {
        g_orig_CreateGfxPso = reinterpret_cast<PFN_CreateGraphicsPipelineState>(vt[10]);
        vt[10] = reinterpret_cast<void*>(&Hook_CreateGraphicsPipelineState);
        DWORD junk = 0;
        VirtualProtect(&vt[10], sizeof(void*), o10, &junk);
        log("[pso] CreateGraphicsPipelineState hooked dev=%p", device);
    }
    {
        ID3D12Device2* dev2 = nullptr;
        auto* dev0 = static_cast<ID3D12Device*>(device);
        if (SUCCEEDED(dev0->QueryInterface(IID_PPV_ARGS(&dev2))) && dev2) {
            DWORD o47 = 0;
            if (VirtualProtect(&vt[47], sizeof(void*), PAGE_READWRITE, &o47)) {
                g_orig_CreatePso = reinterpret_cast<PFN_CreatePipelineState>(vt[47]);
                vt[47] = reinterpret_cast<void*>(&Hook_CreatePipelineState);
                DWORD junk = 0;
                VirtualProtect(&vt[47], sizeof(void*), o47, &junk);
                log("[pso] ID3D12Device2::CreatePipelineState hooked dev=%p", device);
            }
            dev2->Release();
        }
    }
    DWORD o17 = 0;
    if (VirtualProtect(&vt[17], sizeof(void*), PAGE_READWRITE, &o17)) {
        g_orig_CreateCBV = reinterpret_cast<CreateCBVFn>(vt[17]);
        vt[17] = reinterpret_cast<void*>(&hk_CreateCBV);
        DWORD junk = 0;
        VirtualProtect(&vt[17], sizeof(void*), o17, &junk);
        log("[cbv] CreateConstantBufferView hooked dev=%p orig=%p", device,
            (void*)g_orig_CreateCBV);
    }
    // Real game objects only: no throwaway device through sl.interposer.
    DWORD o20 = 0;
    if (VirtualProtect(&vt[20], sizeof(void*), PAGE_READWRITE, &o20)) {
        g_orig_CreateRTV = reinterpret_cast<CreateRTVFn>(vt[20]);
        vt[20] = reinterpret_cast<void*>(&hk_CreateRTV);
        DWORD junk = 0;
        VirtualProtect(&vt[20], sizeof(void*), o20, &junk);
        g_rtv_hook_installed.store(true, std::memory_order_release);
        log("[mirror] real device CreateRTV hooked dev=%p orig=%p",
            device, (void*)g_orig_CreateRTV);
    }
}

}  // namespace detail
}  // namespace cvr
