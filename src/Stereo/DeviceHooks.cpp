#include "Natives/MenuSurface.hpp"
#include "Core/Paths.hpp"
#include "Utils/DebugGate.hpp"
#include "Render/NativePreparation.hpp"
#include "Render/PhysicalOpticsShaders.hpp"
#include "Render/PipelineVariants.hpp"
#include "Render/SceneGpuProfile.hpp"
#include "Hooks/AftermathDiagnostics.hpp"
#include "Render/CommandResources.hpp"
#include "Render/FoveatedDlss.hpp"
#include "Render/FixedVrs.hpp"
#include "Render/MonoCascades.hpp"
#include "Render/HiddenArea.hpp"
#include "Render/HiddenAreaShaders.hpp"
#include "Render/NativeStereoProbe.hpp"
#include "Render/NativeStereoFamilies.hpp"
#include "Render/NativeIaBinding.hpp"
#include "Render/NativeGeometrySnapshots.hpp"
#include "Render/ViewInstancedPipeline.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/NativeConstantCapture.hpp"
#include "Render/ConstantUploadRegistry.hpp"
#include "Render/RegisteredConstantUpload.hpp"
#include "Render/NativeEarlyDepth.hpp"
#include "Render/NativeTextureRead.hpp"
#include "Render/NativeBufferRead.hpp"
#include "Render/NativeCopyWindows.hpp"
#include "Render/NativeCommandRecording.hpp"
#include "Render/NativeSubmissionFrontier.hpp"
#include "Render/NativeTargetHandoff.hpp"
#include "Render/NativeTargetAccess.hpp"
#include "Render/NativeRetainedGeometry.hpp"
#include "Render/NativePairedGeometry.hpp"
#include "Render/NativeMaterialBinding.hpp"
#include "Render/StereoDescriptorAudit.hpp"
#include "Render/StereoGpuProbe.hpp"
#include "Render/StereoSceneState.hpp"
#include <wrl/client.h>
#include <memory>
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
#include "Render/WorldMarkerShader.hpp"
#include "Framegen/Inputs.hpp"
#include "Framegen/GpuTimer.hpp"
#include "Camera/ImagePoseIdentity.hpp"
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
    cvr::stereo::descriptors::MutationScope mutation;
    const uintptr_t ret_abs = reinterpret_cast<uintptr_t>(_ReturnAddress());
    CVR_DIAGNOSTIC(CyberpunkVR_DebugDescHeapCreates++);
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
                CVR_DIAGNOSTIC(CyberpunkVR_DebugDescHeapEnlarged++);
                log("[descheap] enlarged -> %u", g_desc_heap_target);
            }
        }
    }
    const auto result=g_orig_CreateDescriptorHeap(self,use,riid,out);
    if(SUCCEEDED(result) && out && *out)cvr::stereo::descriptors::HeapCreated(self,static_cast<ID3D12DescriptorHeap*>(*out));
    if(SUCCEEDED(result) && use && use->Type==D3D12_DESCRIPTOR_HEAP_TYPE_RTV)rtv_descriptor_heap_created(use->NumDescriptors);
    return result;
}

// --- ExecuteCommandLists probe: count actual GPU command-list executions per
// frame. Definitive test for "does the eye execute?": if execLists/frame ~doubles
// under stereo, the eye's command lists really run on the GPU. Queue vtable slot
// 10 = ExecuteCommandLists; device vtable slot 8 = CreateCommandQueue.
bool g_enable_exec_probe = true;
std::atomic<uint64_t> g_exec_total{0};        // cumulative command lists executed
std::atomic<uint64_t> g_exec_last_frame{0};
std::atomic<uint64_t> g_exec_frame_ctr{0};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugExecTotal    = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugExecPerFrame = 0;

using PFN_ExecuteCommandLists =
    void (STDMETHODCALLTYPE*)(ID3D12CommandQueue*, UINT, ID3D12CommandList* const*);
using PFN_CreateCommandQueue =
    HRESULT (STDMETHODCALLTYPE*)(ID3D12Device*, const D3D12_COMMAND_QUEUE_DESC*, REFIID, void**);
struct QueueExecuteHook {void** table{};PFN_ExecuteCommandLists original{};};
static std::array<QueueExecuteHook,16> g_queue_execute_hooks{};
static std::atomic<uint32_t> g_queue_execute_count{};
static std::mutex g_queue_execute_mutex;
static void ReadObservationUnavailable(){
    cvr::stereo::target_access::ObserverUnavailable();
    cvr::stereo::texture_read::ObserverUnavailable();cvr::stereo::buffer_read::ObserverUnavailable();
}
PFN_ExecuteCommandLists command_queue_original_execute(ID3D12CommandQueue* queue){
    if(!queue)return nullptr;auto** table=*reinterpret_cast<void***>(queue);
    for(uint32_t i=0,n=g_queue_execute_count.load(std::memory_order_acquire);i<n;++i)
        if(g_queue_execute_hooks[i].table==table)return g_queue_execute_hooks[i].original;
    return nullptr;
}
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
// Set while the vrcam CopyToTexture (sub_1403FE078) node is recording on THIS thread: the crop
// pass whose viewport comes from a render-res SOURCE resource (not VP+0x34), so the VP-swap alone
// can't move it. While set, hk_RSSetViewports/Scissor upscale the render-res (1418) viewport to
// output (2444) -> the copy fills the whole 2444 target. Scoped to just this one node + vrcam.
thread_local bool t_copytotex = false;
// (removed: investigation-only viewport/blit diagnostics -- crop root FOUND = raster tonemap
//  sub_140557204 gated by group 20; fixed natively in Detour_RenderRes via view+0x17D0 match-main.
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
// The metadata cache now lives in Capture.cpp. Active bindings refresh its LRU
// position, and swapchain buffers are pinned independently of descriptor age.
// Resource/dimension reads are one locked snapshot; an atomic handle alone did
// not protect the plain fields from a concurrent descriptor replacement.
std::atomic<uint32_t> g_rtv_dim_count{0};
std::atomic<bool> g_rtv_dim_wrapped_logged{false};
// How often the HUD node binds a target, and how often we cannot say what that bind points at.
// The second number rising with the first is the map above failing to answer, which is the whole
// difference between "the HUD moved" and "we went blind to it".
std::atomic<uint64_t> g_hud_node_binds{0};
std::atomic<uint64_t> g_hud_node_unresolved{0};
// d12_present_thread and d12_submit_mirror_copy moved to src/Stereo/Mirror.cpp; both are declared
// in Stereo/StereoInternal.hpp. A `static` forward declaration here would promise a definition in
// THIS file -- the shape that has now appeared in six extractions.

// Submission cost, QPC ticks summed (the tail of each view's graph is a chain of submissions):
// [0] calls, [1] lists, [2] HookedExecuteCommandLists before it calls on, [3] this hook before the
// driver call, [4] the driver call, [5] this hook after it. Hooks/DepthCapture.cpp writes [2].
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_EclTiming[8]{};
static uint64_t EclTicks(){LARGE_INTEGER t{};QueryPerformanceCounter(&t);return uint64_t(t.QuadPart);}

static void STDMETHODCALLTYPE Hook_ExecuteCommandLists(
        ID3D12CommandQueue* self, UINT n, ID3D12CommandList* const* lists) {
    const uint64_t eclStart=EclTicks();
    CyberpunkVR_EclTiming[0].fetch_add(1,std::memory_order_relaxed);
    CyberpunkVR_EclTiming[1].fetch_add(n,std::memory_order_relaxed);
    const auto caller=reinterpret_cast<uintptr_t>(_ReturnAddress());
    const auto original=command_queue_original_execute(self);if(!original)return;
    cvr::stereo::constants::ObserveQueueApi(self);
    CVR_DIAGNOSTIC(CyberpunkVR_DebugExecTotal = g_exec_total.fetch_add(n, std::memory_order_relaxed) + n);
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
    {
        std::lock_guard submissionLock(cvr::camera::ImageSubmissionMutex());
        if(self==g_game_queue)cvr::framegen::BeforeGameCommands(self);
        cvr::foveated::BeforeSubmit(self,n,lists);
        auto resources = cvr::gpu::PrepareCommandResources(n, lists);
        auto sceneProfile=cvr::gpu::scene_profile::BeforeSubmit(n,lists);
        cvr::stereo::gpu_probe::BeforeSubmit(self,n,lists);
        cvr::stereo::texture_read::BeforeSubmit(self,n,lists);
        cvr::stereo::buffer_read::BeforeSubmit(self,n,lists);
        cvr::stereo::native_recording::BeforeSubmit(self,n,lists,caller);
        cvr::stereo::target_handoff::BeforeSubmit(self,n,lists);
        cvr::stereo::submission_frontier::ObserveSubmit(self,n,lists);
        const bool retainedSerialize=!cvr::stereo::probe::internalCommands && cvr::stereo::retained_geometry::Active() &&
            cvr::stereo::retained_geometry::BeforeSubmit(self,n,lists);
        const bool pairedSerialize=!cvr::stereo::probe::internalCommands && cvr::stereo::paired_geometry::Active() &&
            cvr::stereo::paired_geometry::BeforeSubmit(self,n,lists);
        const bool serialize=retainedSerialize || pairedSerialize;
        const UINT currentBoundary=retainedSerialize && serialize?
            cvr::stereo::retained_geometry::CurrentBoundary(self,n,lists):0;
        cvr::stereo::retained_geometry::OrderedExecution(self,n,lists,serialize);
        cvr::stereo::early_depth::BeforeSubmit(self,n,lists);
        auto geometrySnapshots=cvr::stereo::geometry_snapshots::BeforeSubmit(n,lists);
        cvr::stereo::descriptors::BeforeSubmit(n,lists);
        {
            // Do not hold the queue observer's ordering mutex while acquiring
            // CommandResources: its submission path can itself signal fences.
            std::unique_lock order(cvr::stereo::constants::RegisteredQueueOrderMutex(),std::defer_lock);
            if(cvr::stereo::constants::RegisteredTracking() || CyberpunkVR_PreparationAllStages.load(std::memory_order_relaxed))order.lock();
            if(order.owns_lock())cvr::stereo::constants::RefreshRegisteredFences();
            const UINT prefix=currentBoundary?currentBoundary:n;
            cvr::stereo::constants::SubmitRegistered(self,prefix,lists);
            cvr::stereo::constants::BeforeSubmit(prefix,lists);
            cvr::stereo::preparation::SubmitLists(self,prefix,lists,order.owns_lock());
            if(serialize && lists)for(UINT i=0;i<n;++i){
                cvr::optics::camera::Execute(self,1,lists+i,original);
                if(currentBoundary && i+1==currentBoundary){
                    cvr::stereo::texture_read::Submitted(self,currentBoundary,lists);
                    cvr::stereo::buffer_read::Submitted(self,currentBoundary,lists);
                    cvr::stereo::native_recording::Submitted(self,currentBoundary,lists);
                    cvr::stereo::target_handoff::Submitted(self,currentBoundary,lists);
                    const bool relock=order.owns_lock();if(relock)order.unlock();
                    cvr::stereo::retained_geometry::CurrentPrefixExecuted(currentBoundary);
                    cvr::stereo::retained_geometry::Submitted(self,currentBoundary,lists);
                    cvr::stereo::retained_geometry::SubmitCurrent(self);
                    if(relock)order.lock();
                    cvr::stereo::constants::SubmitRegistered(self,n-currentBoundary,lists+currentBoundary);
                    cvr::stereo::constants::BeforeSubmit(n-currentBoundary,lists+currentBoundary);
                    cvr::stereo::preparation::SubmitLists(self,n-currentBoundary,lists+currentBoundary,order.owns_lock());
                }
            }
            else {
                const uint64_t driverStart=EclTicks();
                CyberpunkVR_EclTiming[3].fetch_add(driverStart-eclStart,std::memory_order_relaxed);
                cvr::optics::camera::Execute(self,n,lists,original);
                const uint64_t driverEnd=EclTicks();
                CyberpunkVR_EclTiming[4].fetch_add(driverEnd-driverStart,std::memory_order_relaxed);
                CyberpunkVR_EclTiming[5].fetch_sub(driverEnd,std::memory_order_relaxed);   // + end, below
            }
            cvr::stereo::texture_read::Submitted(self,n-currentBoundary,lists?lists+currentBoundary:nullptr);
            cvr::stereo::buffer_read::Submitted(self,n-currentBoundary,lists?lists+currentBoundary:nullptr);
            cvr::stereo::native_recording::Submitted(self,n-currentBoundary,lists?lists+currentBoundary:nullptr);
            cvr::stereo::target_handoff::Submitted(self,n-currentBoundary,lists?lists+currentBoundary:nullptr);
        }
        if(currentBoundary)cvr::stereo::retained_geometry::CurrentSuffixExecuted(n-currentBoundary);
        if(!cvr::stereo::probe::internalCommands && cvr::stereo::retained_geometry::Active()){
            cvr::stereo::retained_geometry::Submitted(self,n,lists);
            cvr::stereo::retained_geometry::SubmitCurrent(self);
        }
        if(!cvr::stereo::probe::internalCommands && cvr::stereo::paired_geometry::Active())
            cvr::stereo::paired_geometry::Submitted(self,n,lists);
        if(lists)for(UINT i=0;i<n;++i)cvr::stereo::material_binding::Reset(static_cast<ID3D12GraphicsCommandList*>(lists[i]));
        cvr::gpu::SubmitCommandResources(self, std::move(resources));
        cvr::stereo::geometry_snapshots::Submitted(self,std::move(geometrySnapshots));
        cvr::gpu::scene_profile::Submitted(self,std::move(sceneProfile));
        cvr::stereo::gpu_probe::Submitted(self,n,lists);
        cvr::stereo::early_depth::Submitted(self,n,lists);
        cvr::camera::CommitImagePoseLists(self,n,lists);
        cvr::framegen::Submitted(self,n,lists);
        cvr::menu_surface::Submitted(self,n,lists);
        if(!serialize)CyberpunkVR_EclTiming[5].fetch_add(EclTicks(),std::memory_order_relaxed);
    }
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
    std::lock_guard lock(g_queue_execute_mutex);
    if(command_queue_original_execute(static_cast<ID3D12CommandQueue*>(queue)))return;
    const auto count=g_queue_execute_count.load(std::memory_order_relaxed);
    if(count>=g_queue_execute_hooks.size()){ReadObservationUnavailable();return;}
    void** vt = *reinterpret_cast<void***>(queue);
    DWORD oldp = 0;
    if (VirtualProtect(&vt[10], sizeof(void*), PAGE_READWRITE, &oldp)) {
        const auto original=reinterpret_cast<PFN_ExecuteCommandLists>(vt[10]);
        g_queue_execute_hooks[count]={vt,original};
        g_queue_execute_count.store(count+1,std::memory_order_release);
        InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vt+10),reinterpret_cast<void*>(&Hook_ExecuteCommandLists));
        DWORD junk = 0;
        VirtualProtect(&vt[10], sizeof(void*), oldp, &junk);
        log("[exec] ExecuteCommandLists hooked (queue=%p orig=%p)",
            queue, (void*)original);
    }else{
        ReadObservationUnavailable();
    }
}

static HRESULT STDMETHODCALLTYPE Hook_CreateCommandQueue(
        ID3D12Device* self, const D3D12_COMMAND_QUEUE_DESC* desc, REFIID riid, void** out) {
    HRESULT hr = g_orig_CreateCommandQueue(self, desc, riid, out);
    if (SUCCEEDED(hr) && out && *out) {
        auto* queue = reinterpret_cast<ID3D12CommandQueue*>(*out);
        cvr::stereo::constants::ObserveQueueApi(queue);
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
// Every command-list vtable patched so far, for subsystems that chain further slots at runtime
// (Render/StageReplay.cpp). Entries are published before their count, never removed.
uint32_t command_list_hook_entries(const CommandListVtableHook** out,uint32_t max) {
    const uint32_t count=g_command_list_vtable_hook_count.load(std::memory_order_acquire);
    const uint32_t n=count<max?count:max;
    for(uint32_t i=0;i<n;++i)out[i]=&g_command_list_vtable_hooks[i];
    return n;
}
PFN_OMSetRenderTargets command_list_original_om(
        ID3D12GraphicsCommandList* command_list) {
    const CommandListVtableHook* e = command_list_hook_entry(command_list);
    return e ? e->original : nullptr;
}

static void STDMETHODCALLTYPE ProbeHeaps(ID3D12GraphicsCommandList* list,UINT count,ID3D12DescriptorHeap* const* heaps) {
    const auto* e=command_list_hook_entry(list);if(!e || !e->heaps_original)return;
    e->heaps_original(list,count,heaps);cvr::stereo::scene_state::Heaps(list,count,heaps);
    cvr::stereo::native_probe::Heaps(list,count,heaps);
    if(cvr::stereo::descriptors::Enabled()){Microsoft::WRL::ComPtr<ID3D12Device> device;
        if(SUCCEEDED(list->GetDevice(IID_PPV_ARGS(&device))))cvr::stereo::descriptors::HeapsBound(device.Get(),count,heaps);}
}
static void STDMETHODCALLTYPE ProbeTopology(ID3D12GraphicsCommandList* list,D3D12_PRIMITIVE_TOPOLOGY topology) {
    const auto* e=command_list_hook_entry(list);if(!e || !e->topology_original)return;
    e->topology_original(list,topology);cvr::stereo::scene_state::Topology(list,topology);
}
static void STDMETHODCALLTYPE ProbeStencil(ID3D12GraphicsCommandList* list,UINT stencil) {
    const auto* e=command_list_hook_entry(list);if(!e || !e->stencil_original)return;
    e->stencil_original(list,stencil);cvr::stereo::scene_state::Stencil(list,stencil);
}
static void STDMETHODCALLTYPE ProbeBlendFactor(ID3D12GraphicsCommandList* list,const FLOAT* factors) {
    const auto* e=command_list_hook_entry(list);if(!e || !e->blend_factor_original)return;
    e->blend_factor_original(list,factors);cvr::stereo::scene_state::BlendFactor(list,factors);
}
static void STDMETHODCALLTYPE ProbeGraphicsRoot(ID3D12GraphicsCommandList* list,ID3D12RootSignature* root) {
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->groot_original)return;
    entry->groot_original(list,root);cvr::stereo::native_probe::RootSignature(list,root);
}
static void STDMETHODCALLTYPE ProbeGraphicsTable(ID3D12GraphicsCommandList* list,UINT index,D3D12_GPU_DESCRIPTOR_HANDLE table) {
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->grootdt_original)return;
    entry->grootdt_original(list,index,table);cvr::stereo::native_probe::RootTable(list,index,table);
}
static void STDMETHODCALLTYPE ProbeGraphicsCbv(ID3D12GraphicsCommandList* list,UINT index,D3D12_GPU_VIRTUAL_ADDRESS address) {
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->grootcbv_original)return;
    entry->grootcbv_original(list,index,address);cvr::stereo::native_probe::RootCbv(list,index,address);
}
static void STDMETHODCALLTYPE ProbeIndexBuffer(ID3D12GraphicsCommandList* list,const D3D12_INDEX_BUFFER_VIEW* view) {
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->iaib_original)return;
    entry->iaib_original(list,view);cvr::stereo::native_probe::IndexBuffer(list,view);
}
static void STDMETHODCALLTYPE ProbePredication(ID3D12GraphicsCommandList* list,ID3D12Resource* buffer,UINT64 offset,D3D12_PREDICATION_OP operation) {
    if(buffer && !cvr::stereo::probe::internalCommands && cvr::stereo::retained_geometry::Active())cvr::stereo::retained_geometry::Flush(list,16);
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->predication_original)return;
    entry->predication_original(list,buffer,offset,operation);cvr::stereo::native_probe::Predication(list,buffer);
}
static void STDMETHODCALLTYPE ProbeShadingRate(ID3D12GraphicsCommandList5* list,D3D12_SHADING_RATE rate,const D3D12_SHADING_RATE_COMBINER* combiners) {
    const auto* e=command_list_hook_entry(list);if(!e || !e->shading_rate_original)return;
    cvr::vrs::Restore(list);
    e->shading_rate_original(list,rate,combiners);cvr::stereo::scene_state::ShadingRate(list,rate,combiners);
    cvr::vrs::ShadingRate(list,rate,combiners);cvr::optics::ShadingRateSet(list,rate,combiners);
}
static void STDMETHODCALLTYPE ProbeShadingImage(ID3D12GraphicsCommandList5* list,ID3D12Resource* image) {
    const auto* e=command_list_hook_entry(list);if(!e || !e->shading_image_original)return;
    cvr::vrs::Restore(list);
    e->shading_image_original(list,image);cvr::stereo::scene_state::ShadingRateImage(list,image);
    cvr::vrs::ShadingImage(list,image);
}
static void STDMETHODCALLTYPE VrsClearState(ID3D12GraphicsCommandList* list,ID3D12PipelineState* pso) {
    cvr::stereo::target_handoff::Unknown(list,1);
    cvr::stereo::target_access::Unknown(list,1);
    cvr::stereo::copy_windows::Unknown(list);
    const auto* e=command_list_hook_entry(list);if(!e || !e->clear_state_original)return;
    if(!cvr::stereo::probe::internalCommands && cvr::stereo::retained_geometry::Active())cvr::stereo::retained_geometry::Flush(list,1);
    e->clear_state_original(list,pso);cvr::vrs::Reset(list,e->shading_rate_original!=nullptr,pso);cvr::optics::ListReset(list);
    census_pipeline_reset(pso);
    cvr::stereo::families::Reset(list,pso);
    cvr::stereo::native_probe::Reset(list,pso,e->shading_rate_original!=nullptr,false);
}
static void STDMETHODCALLTYPE VrsExecuteBundle(ID3D12GraphicsCommandList* list,ID3D12GraphicsCommandList* bundle) {
    cvr::stereo::target_access::Unknown(list,2);
    cvr::stereo::target_handoff::Unknown(list,2);
    cvr::stereo::copy_windows::Unknown(list);
    const auto* e=command_list_hook_entry(list);if(!e || !e->bundle_original)return;
    if(!cvr::stereo::probe::internalCommands && cvr::stereo::retained_geometry::Active())cvr::stereo::retained_geometry::Flush(list,2);
    cvr::vrs::Restore(list);
    e->bundle_original(list,bundle);cvr::vrs::Invalidate(list);
    census_pipeline_reset(nullptr);
    cvr::stereo::families::Reset(list,nullptr);
    cvr::stereo::native_probe::Reset(list,nullptr,false,false);
}
static HRESULT STDMETHODCALLTYPE ProfileClose(ID3D12GraphicsCommandList* list) {
    const auto* entry=command_list_hook_entry(list);
    if(!entry || !entry->close_original)return E_FAIL;
    if(!cvr::stereo::probe::internalCommands && cvr::stereo::retained_geometry::Active())cvr::stereo::retained_geometry::BeforeClose(list);
    if(list->GetType()==D3D12_COMMAND_LIST_TYPE_COPY){const auto result=entry->close_original(list);
        cvr::stereo::target_handoff::Close(list,result);cvr::stereo::target_access::Close(list,result);return result;}
    cvr::stereo::early_depth::Closing(list);
    cvr::vrs::Restore(list);
    cvr::cascades::Close(list);
    cvr::gpu::scene_profile::Close(list);
    const auto result=entry->close_original(list);
    cvr::stereo::target_handoff::Close(list,result);
    cvr::stereo::target_access::Close(list,result);
    if(SUCCEEDED(result))cvr::stereo::material_binding::Reset(list);
    return result;
}
static void STDMETHODCALLTYPE CascadeDiscard(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,const D3D12_DISCARD_REGION* region) {
    if(!cvr::stereo::probe::internalCommands && cvr::stereo::retained_geometry::Active())cvr::stereo::retained_geometry::BeforeResources(list,nullptr,resource,3);
    cvr::stereo::buffer_read::Written(nullptr,resource);
    cvr::stereo::texture_read::Written(nullptr,resource);
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->discard_original)return;
    const bool clearNode=g_exe_base && t_current_node_work==reinterpret_cast<uintptr_t>(g_exe_base)+CASCADE_CLEAR_NODE_RVA;
    if(cvr::cascades::Discard(resource,t_view_side,clearNode))return;
    cvr::stereo::target_handoff::Written(list,resource,3);
    cvr::stereo::target_access::Written(list,resource,3);
    cvr::stereo::early_depth::InstanceWrite(list,resource);
    cvr::stereo::constants::InvalidateRegistered(list,resource);
    entry->discard_original(list,resource,region);
}
static void STDMETHODCALLTYPE CascadeCopyTexture(ID3D12GraphicsCommandList* list,const D3D12_TEXTURE_COPY_LOCATION* dest,UINT x,UINT y,UINT z,const D3D12_TEXTURE_COPY_LOCATION* source,const D3D12_BOX* box) {
    if(!cvr::stereo::probe::internalCommands && cvr::stereo::retained_geometry::Active())
        cvr::stereo::retained_geometry::BeforeResources(list,source?source->pResource:nullptr,dest?dest->pResource:nullptr,4);
    if(dest)cvr::stereo::buffer_read::Written(list,dest->pResource);
    if(dest)cvr::stereo::texture_read::Written(list,dest->pResource);
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->copytex)return;
    if(dest)cvr::cascades::Write(dest->pResource);
    if(dest)cvr::stereo::constants::InvalidateRegistered(list,dest->pResource);
    cvr::stereo::target_handoff::Written(list,dest?dest->pResource:nullptr,4);
    cvr::stereo::target_access::Read(list,source?source->pResource:nullptr,4,
        source && source->Type==D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX?source->SubresourceIndex:D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES);
    cvr::stereo::target_access::Written(list,dest?dest->pResource:nullptr,4,
        dest && dest->Type==D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX?dest->SubresourceIndex:D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES);
    entry->copytex(list,dest,x,y,z,source,box);
}
static void STDMETHODCALLTYPE ObserveQueryResolve(ID3D12GraphicsCommandList* list,ID3D12QueryHeap* heap,D3D12_QUERY_TYPE type,UINT first,UINT count,ID3D12Resource* destination,UINT64 offset){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->resolve_query_original)return;
    cvr::stereo::buffer_read::Written(nullptr,destination);
    cvr::stereo::target_access::Written(list,destination,12);
    entry->resolve_query_original(list,heap,type,first,count,destination,offset);
}
static void STDMETHODCALLTYPE ObserveImmediateWrite(ID3D12GraphicsCommandList2* list,UINT count,const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER* parameters,const D3D12_WRITEBUFFERIMMEDIATE_MODE* modes){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->write_immediate_original)return;
    if(count)cvr::stereo::target_access::ImmediateWrite(list,count,parameters,modes,reinterpret_cast<uintptr_t>(_ReturnAddress()));
    if(count && !parameters)cvr::stereo::buffer_read::ObserverUnavailable();
    else for(UINT i=0;i<count;++i)cvr::stereo::buffer_read::WrittenAddress(parameters[i].Dest,4);
    entry->write_immediate_original(list,count,parameters,modes);
}
static void STDMETHODCALLTYPE ObserveEnhancedBarrier(ID3D12GraphicsCommandList7* list,UINT count,const D3D12_BARRIER_GROUP* groups){
    if(count)cvr::stereo::target_handoff::Unknown(list,5);
    if(count)cvr::stereo::target_access::Unknown(list,5);
    if(count)cvr::stereo::copy_windows::Each([](auto& window){window.Write(0);});
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->enhanced_barrier_original)return;
    if(!cvr::stereo::probe::internalCommands && cvr::stereo::retained_geometry::Active())cvr::stereo::retained_geometry::Flush(list,13);
    if(count && !groups)ReadObservationUnavailable();
    else for(UINT i=0;i<count;++i){const auto& group=groups[i];
        if(group.Type==D3D12_BARRIER_TYPE_BUFFER && group.pBufferBarriers){
            for(UINT j=0;j<group.NumBarriers;++j)cvr::stereo::buffer_read::Written(nullptr,group.pBufferBarriers[j].pResource);
        }else if(group.Type==D3D12_BARRIER_TYPE_TEXTURE && group.pTextureBarriers){
            for(UINT j=0;j<group.NumBarriers;++j)cvr::stereo::texture_read::Written(nullptr,group.pTextureBarriers[j].pResource);
        }else if(group.NumBarriers)ReadObservationUnavailable();
    }
    const auto hiddenBase=reinterpret_cast<uintptr_t>(g_exe_base);
    const uint32_t hiddenNode=hiddenBase && t_current_node_work>hiddenBase?uint32_t(t_current_node_work-hiddenBase):0;
    if(entry->cleardsv_original)cvr::hidden_area::BeforeEnhancedBarriers(list,count,groups,t_view_side,hiddenNode,entry->cleardsv_original);
    entry->enhanced_barrier_original(list,count,groups);
    if(entry->cleardsv_original)cvr::hidden_area::AfterEnhancedBarriers(list,count,groups,t_view_side,hiddenNode,entry->cleardsv_original);
}
static void STDMETHODCALLTYPE ObserveStreamOutput(ID3D12GraphicsCommandList* list,UINT first,UINT count,const D3D12_STREAM_OUTPUT_BUFFER_VIEW* views){
    cvr::stereo::copy_windows::Unknown(list);
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->stream_output_original)return;
    if(count)cvr::stereo::target_access::Unknown(list,14);
    if(views)for(UINT i=0;i<count;++i){
        if(views[i].BufferLocation)cvr::stereo::buffer_read::WrittenAddress(views[i].BufferLocation,views[i].SizeInBytes);
        if(views[i].BufferFilledSizeLocation)cvr::stereo::buffer_read::WrittenAddress(views[i].BufferFilledSizeLocation,8);
    }
    entry->stream_output_original(list,first,count,views);
}
static void STDMETHODCALLTYPE ObserveCopyTiles(ID3D12GraphicsCommandList* list,ID3D12Resource* tiled,const D3D12_TILED_RESOURCE_COORDINATE* coordinate,
    const D3D12_TILE_REGION_SIZE* size,ID3D12Resource* linear,UINT64 offset,D3D12_TILE_COPY_FLAGS flags){
    if(!cvr::stereo::probe::internalCommands && cvr::stereo::retained_geometry::Active())cvr::stereo::retained_geometry::BeforeResources(list,tiled,linear,15);
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->copy_tiles_original)return;
    cvr::stereo::target_access::Unknown(list,15);
    if(flags&D3D12_TILE_COPY_FLAG_SWIZZLED_TILED_RESOURCE_TO_LINEAR_BUFFER)cvr::stereo::buffer_read::Written(nullptr,linear);
    if(flags&D3D12_TILE_COPY_FLAG_LINEAR_BUFFER_TO_SWIZZLED_TILED_RESOURCE)cvr::stereo::texture_read::Written(nullptr,tiled);
    if(!(flags&(D3D12_TILE_COPY_FLAG_SWIZZLED_TILED_RESOURCE_TO_LINEAR_BUFFER|D3D12_TILE_COPY_FLAG_LINEAR_BUFFER_TO_SWIZZLED_TILED_RESOURCE)))ReadObservationUnavailable();
    if(flags&D3D12_TILE_COPY_FLAG_LINEAR_BUFFER_TO_SWIZZLED_TILED_RESOURCE)cvr::stereo::target_handoff::Written(list,tiled,6);
    entry->copy_tiles_original(list,tiled,coordinate,size,linear,offset,flags);
}
template<bool Wide>static void STDMETHODCALLTYPE ObserveAtomicCopy(ID3D12GraphicsCommandList1* list,ID3D12Resource* destination,UINT64 destinationOffset,
    ID3D12Resource* source,UINT64 sourceOffset,UINT count,ID3D12Resource* const* dependencies,const D3D12_SUBRESOURCE_RANGE_UINT64* ranges){
    const auto* entry=command_list_hook_entry(list);if(!entry)return;
    const auto original=Wide?entry->atomic_copy64_original:entry->atomic_copy32_original;if(!original)return;
    cvr::stereo::target_access::Unknown(list,16);
    cvr::stereo::buffer_read::Written(nullptr,destination);
    original(list,destination,destinationOffset,source,sourceOffset,count,dependencies,ranges);
}
static void STDMETHODCALLTYPE RetainedClearRtv(ID3D12GraphicsCommandList* list,D3D12_CPU_DESCRIPTOR_HANDLE target,
    const FLOAT* color,UINT count,const D3D12_RECT* rects){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->clear_rtv_original)return;
    if(!cvr::stereo::probe::internalCommands && cvr::stereo::retained_geometry::Active())cvr::stereo::retained_geometry::Flush(list,6);
    cvr::stereo::target_handoff::ClearView(list,target,false);
    cvr::stereo::target_access::ClearView(list,target,false);
    entry->clear_rtv_original(list,target,color,count,rects);
}
static void STDMETHODCALLTYPE ObserveClearUavUint(ID3D12GraphicsCommandList* list,D3D12_GPU_DESCRIPTOR_HANDLE gpu,
    D3D12_CPU_DESCRIPTOR_HANDLE cpu,ID3D12Resource* resource,const UINT* values,UINT count,const D3D12_RECT* rects){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->clear_uav_uint_original)return;
    cvr::stereo::target_handoff::Written(list,resource,11);
    cvr::stereo::target_access::Written(list,resource,11);
    cvr::stereo::buffer_read::Written(list,resource);cvr::stereo::texture_read::Written(list,resource);
    entry->clear_uav_uint_original(list,gpu,cpu,resource,values,count,rects);
}
static void STDMETHODCALLTYPE ObserveClearUavFloat(ID3D12GraphicsCommandList* list,D3D12_GPU_DESCRIPTOR_HANDLE gpu,
    D3D12_CPU_DESCRIPTOR_HANDLE cpu,ID3D12Resource* resource,const FLOAT* values,UINT count,const D3D12_RECT* rects){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->clear_uav_float_original)return;
    cvr::stereo::target_handoff::Written(list,resource,11);
    cvr::stereo::target_access::Written(list,resource,11);
    cvr::stereo::buffer_read::Written(list,resource);cvr::stereo::texture_read::Written(list,resource);
    entry->clear_uav_float_original(list,gpu,cpu,resource,values,count,rects);
}
static void STDMETHODCALLTYPE RetainedResolve(ID3D12GraphicsCommandList* list,ID3D12Resource* destination,UINT destinationIndex,
    ID3D12Resource* source,UINT sourceIndex,DXGI_FORMAT format){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->resolve_subresource_original)return;
    if(!cvr::stereo::probe::internalCommands && cvr::stereo::retained_geometry::Active())
        cvr::stereo::retained_geometry::BeforeResources(list,source,destination,11);
    cvr::stereo::target_handoff::Written(list,destination,7);
    cvr::stereo::target_access::Read(list,source,7);cvr::stereo::target_access::Written(list,destination,7);
    entry->resolve_subresource_original(list,destination,destinationIndex,source,sourceIndex,format);
}
static void STDMETHODCALLTYPE RetainedResolveRegion(ID3D12GraphicsCommandList1* list,ID3D12Resource* destination,UINT destinationIndex,
    UINT x,UINT y,ID3D12Resource* source,UINT sourceIndex,const D3D12_RECT* rect,DXGI_FORMAT format,D3D12_RESOLVE_MODE mode){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->resolve_subresource_region_original)return;
    if(!cvr::stereo::probe::internalCommands && cvr::stereo::retained_geometry::Active())
        cvr::stereo::retained_geometry::BeforeResources(list,source,destination,11);
    cvr::stereo::target_handoff::Written(list,destination,7);
    cvr::stereo::target_access::Read(list,source,7);cvr::stereo::target_access::Written(list,destination,7);
    entry->resolve_subresource_region_original(list,destination,destinationIndex,x,y,source,sourceIndex,rect,format,mode);
}
static void STDMETHODCALLTYPE RetainedBeginRenderPass(ID3D12GraphicsCommandList4* list,UINT count,
    const D3D12_RENDER_PASS_RENDER_TARGET_DESC* targets,const D3D12_RENDER_PASS_DEPTH_STENCIL_DESC* depth,D3D12_RENDER_PASS_FLAGS flags){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->begin_render_pass_original)return;
    cvr::stereo::retained_geometry::RenderPass(list,true);
    cvr::stereo::target_handoff::Unknown(list,8);
    cvr::stereo::target_access::Unknown(list,8);
    entry->begin_render_pass_original(list,count,targets,depth,flags);
}
static void STDMETHODCALLTYPE RetainedEndRenderPass(ID3D12GraphicsCommandList4* list){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->end_render_pass_original)return;
    entry->end_render_pass_original(list);cvr::stereo::retained_geometry::RenderPass(list,false);
}
template<bool Initialize>static void STDMETHODCALLTYPE ObserveMetaCommand(ID3D12GraphicsCommandList4* list,ID3D12MetaCommand* command,const void* data,SIZE_T bytes){
    const auto* entry=command_list_hook_entry(list);if(!entry)return;
    const auto original=Initialize?entry->initialize_meta_original:entry->execute_meta_original;if(!original)return;
    cvr::stereo::target_access::Unknown(list,17);cvr::stereo::target_handoff::Unknown(list,17);original(list,command,data,bytes);
}
static void STDMETHODCALLTYPE ObserveRayBuild(ID3D12GraphicsCommandList4* list,const D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC* desc,
    UINT count,const D3D12_RAYTRACING_ACCELERATION_STRUCTURE_POSTBUILD_INFO_DESC* info){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->build_ray_original)return;
    cvr::stereo::target_access::Unknown(list,18);cvr::stereo::target_handoff::Unknown(list,18);entry->build_ray_original(list,desc,count,info);
}
static void STDMETHODCALLTYPE ObserveRayEmit(ID3D12GraphicsCommandList4* list,const D3D12_RAYTRACING_ACCELERATION_STRUCTURE_POSTBUILD_INFO_DESC* info,
    UINT count,const D3D12_GPU_VIRTUAL_ADDRESS* addresses){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->emit_ray_original)return;
    cvr::stereo::target_access::Unknown(list,19);cvr::stereo::target_handoff::Unknown(list,19);entry->emit_ray_original(list,info,count,addresses);
}
static void STDMETHODCALLTYPE ObserveRayCopy(ID3D12GraphicsCommandList4* list,D3D12_GPU_VIRTUAL_ADDRESS to,D3D12_GPU_VIRTUAL_ADDRESS from,
    D3D12_RAYTRACING_ACCELERATION_STRUCTURE_COPY_MODE mode){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->copy_ray_original)return;
    cvr::stereo::target_access::Unknown(list,20);cvr::stereo::target_handoff::Unknown(list,20);entry->copy_ray_original(list,to,from,mode);
}
static void STDMETHODCALLTYPE ObserveRayDispatch(ID3D12GraphicsCommandList4* list,const D3D12_DISPATCH_RAYS_DESC* desc){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->dispatch_ray_original)return;
    census_dispatch_rays(desc);
    cvr::stereo::target_access::Unknown(list,21);cvr::stereo::target_handoff::Unknown(list,21);entry->dispatch_ray_original(list,desc);
}
// Without the native probe: the census alone (which ray passes run for which view).
static void STDMETHODCALLTYPE CensusRayDispatch(ID3D12GraphicsCommandList4* list,const D3D12_DISPATCH_RAYS_DESC* desc){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->dispatch_ray_original)return;
    census_dispatch_rays(desc);entry->dispatch_ray_original(list,desc);
}
static void STDMETHODCALLTYPE ObserveMeshDispatch(ID3D12GraphicsCommandList6* list,UINT x,UINT y,UINT z){
    const auto* entry=command_list_hook_entry(list);if(!entry || !entry->dispatch_mesh_original)return;
    cvr::stereo::target_access::Unknown(list,22);cvr::stereo::target_handoff::Unknown(list,22);entry->dispatch_mesh_original(list,x,y,z);
}
static void patch_command_list_vtable(void* command_list) {
    if (!command_list) return;
    void** vtable = *reinterpret_cast<void***>(command_list);
    std::lock_guard<std::mutex> lock(g_command_list_vtable_hook_mtx);
    const uint32_t count = g_command_list_vtable_hook_count.load(std::memory_order_relaxed);
    for (uint32_t i=0;i<count;++i) if(g_command_list_vtable_hooks[i].vtable==vtable)return;
    if(count>=g_command_list_vtable_hooks.size()){ReadObservationUnavailable();return;}
    const bool probeEnabled=cvr::stereo::native_probe::ObserveBindings() || cvr::stereo::native_probe::Enabled() || cvr::stereo::families::Enabled();
    if(!probeEnabled)cvr::stereo::target_access::ObserverUnavailable();
    Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList5> list5;
    const auto list5Result=static_cast<ID3D12GraphicsCommandList*>(command_list)->QueryInterface(IID_PPV_ARGS(&list5));
    const bool sameList5=SUCCEEDED(list5Result) && list5.Get()==command_list && *reinterpret_cast<void***>(list5.Get())==vtable;
    Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList1> list1;
    Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList2> list2;
    Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList4> list4;
    Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList7> list7;
    const auto version1=static_cast<ID3D12GraphicsCommandList*>(command_list)->QueryInterface(IID_PPV_ARGS(&list1));
    const auto version2=static_cast<ID3D12GraphicsCommandList*>(command_list)->QueryInterface(IID_PPV_ARGS(&list2));
    const auto version4=static_cast<ID3D12GraphicsCommandList*>(command_list)->QueryInterface(IID_PPV_ARGS(&list4));
    const auto version7=static_cast<ID3D12GraphicsCommandList*>(command_list)->QueryInterface(IID_PPV_ARGS(&list7));
    const bool same1=SUCCEEDED(version1) && list1.Get()==command_list && *reinterpret_cast<void***>(list1.Get())==vtable;
    const bool same2=SUCCEEDED(version2) && list2.Get()==command_list && *reinterpret_cast<void***>(list2.Get())==vtable;
    const bool same4=SUCCEEDED(version4) && list4.Get()==command_list && *reinterpret_cast<void***>(list4.Get())==vtable;
    const bool same7=SUCCEEDED(version7) && list7.Get()==command_list && *reinterpret_cast<void***>(list7.Get())==vtable;
    Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList6> list6;
    const auto version6=static_cast<ID3D12GraphicsCommandList*>(command_list)->QueryInterface(IID_PPV_ARGS(&list6));
    const bool same6=SUCCEEDED(version6) && list6.Get()==command_list && *reinterpret_cast<void***>(list6.Get())==vtable;
    if(!same6 && version6!=E_NOINTERFACE)ReadObservationUnavailable();
    if((!same1 && version1!=E_NOINTERFACE) || (!same2 && version2!=E_NOINTERFACE) || (!same4 && version4!=E_NOINTERFACE) || (!same7 && version7!=E_NOINTERFACE))ReadObservationUnavailable();

    // Other render workers already use this shared vtable. Publish every
    // original BEFORE exposing even the first detour. The old per-slot patch
    // published the record last; a concurrent detour then returned without
    // recording its native command (including barriers, root tables and draws).
    const CommandListVtableHook entry{
        vtable,
        reinterpret_cast<PFN_OMSetRenderTargets>(vtable[46]),
        reinterpret_cast<PFN_ResourceBarrier>(vtable[26]),
        reinterpret_cast<PFN_ResourceBarrier>(vtable[26]),
        reinterpret_cast<PFN_CopyResource>(vtable[17]),
        reinterpret_cast<PFN_CopyTextureRegion>(vtable[16]),
        reinterpret_cast<PFN_ExecuteIndirect>(vtable[59]),
        reinterpret_cast<PFN_DrawInstanced>(vtable[12]),
        reinterpret_cast<PFN_DrawIndexedInstanced>(vtable[13]),
        reinterpret_cast<PFN_CopyBufferRegion>(vtable[15]),
        reinterpret_cast<PFN_Dispatch>(vtable[14]),
        reinterpret_cast<PFN_RSSetViewports>(vtable[21]),
        reinterpret_cast<PFN_RSSetScissorRects>(vtable[22]),
        reinterpret_cast<PFN_GfxReset>(vtable[10]),
        reinterpret_cast<PFN_SetPipelineState>(vtable[25]),
        reinterpret_cast<PFN_IASetVertexBuffers>(vtable[44]),
        reinterpret_cast<PFN_ClearDepthStencilView>(vtable[47]),
        reinterpret_cast<PFN_SetComputeRootSignature>(vtable[29]),
        reinterpret_cast<PFN_SetComputeRootDescriptorTable>(vtable[31]),
        reinterpret_cast<PFN_SetComputeRootSignature>(vtable[30]),
        reinterpret_cast<PFN_SetComputeRootDescriptorTable>(vtable[32]),
        reinterpret_cast<PFN_SetGraphicsRootCbv>(vtable[38]),
        reinterpret_cast<PFN_IASetIndexBuffer>(vtable[43]),
        reinterpret_cast<PFN_SetPredication>(vtable[55]),
        reinterpret_cast<PFN_ProbeHeaps>(vtable[28]),reinterpret_cast<PFN_ProbeTopology>(vtable[20]),reinterpret_cast<PFN_ProbeStencil>(vtable[24]),
        sameList5?reinterpret_cast<PFN_ProbeShadingRate>(vtable[77]):nullptr,
        sameList5?reinterpret_cast<PFN_ProbeShadingImage>(vtable[78]):nullptr,
        probeEnabled && (sameList5 || list5Result==E_NOINTERFACE),
        reinterpret_cast<PFN_GfxClose>(vtable[9]),
        reinterpret_cast<decltype(CommandListVtableHook::clear_state_original)>(vtable[11]),
        reinterpret_cast<decltype(CommandListVtableHook::bundle_original)>(vtable[27]),
        reinterpret_cast<decltype(CommandListVtableHook::discard_original)>(vtable[51]),
        reinterpret_cast<decltype(CommandListVtableHook::blend_factor_original)>(vtable[23]),
        reinterpret_cast<decltype(CommandListVtableHook::resolve_query_original)>(vtable[54]),
        same2?reinterpret_cast<decltype(CommandListVtableHook::write_immediate_original)>(vtable[66]):nullptr,
        same7?reinterpret_cast<decltype(CommandListVtableHook::enhanced_barrier_original)>(vtable[80]):nullptr,
        reinterpret_cast<decltype(CommandListVtableHook::stream_output_original)>(vtable[45]),
        reinterpret_cast<decltype(CommandListVtableHook::copy_tiles_original)>(vtable[18]),
        same1?reinterpret_cast<decltype(CommandListVtableHook::atomic_copy32_original)>(vtable[60]):nullptr,
        same1?reinterpret_cast<decltype(CommandListVtableHook::atomic_copy64_original)>(vtable[61]):nullptr,
        reinterpret_cast<decltype(CommandListVtableHook::clear_rtv_original)>(vtable[48]),
        reinterpret_cast<decltype(CommandListVtableHook::resolve_subresource_original)>(vtable[19]),
        same1?reinterpret_cast<decltype(CommandListVtableHook::resolve_subresource_region_original)>(vtable[64]):nullptr,
        same4?reinterpret_cast<decltype(CommandListVtableHook::begin_render_pass_original)>(vtable[68]):nullptr,
        same4?reinterpret_cast<decltype(CommandListVtableHook::end_render_pass_original)>(vtable[69]):nullptr,
        reinterpret_cast<decltype(CommandListVtableHook::clear_uav_uint_original)>(vtable[49]),
        reinterpret_cast<decltype(CommandListVtableHook::clear_uav_float_original)>(vtable[50]),
        same4?reinterpret_cast<decltype(CommandListVtableHook::initialize_meta_original)>(vtable[70]):nullptr,
        same4?reinterpret_cast<decltype(CommandListVtableHook::execute_meta_original)>(vtable[71]):nullptr,
        same4?reinterpret_cast<decltype(CommandListVtableHook::build_ray_original)>(vtable[72]):nullptr,
        same4?reinterpret_cast<decltype(CommandListVtableHook::emit_ray_original)>(vtable[73]):nullptr,
        same4?reinterpret_cast<decltype(CommandListVtableHook::copy_ray_original)>(vtable[74]):nullptr,
        same4?reinterpret_cast<decltype(CommandListVtableHook::dispatch_ray_original)>(vtable[76]):nullptr,
        same6?reinterpret_cast<decltype(CommandListVtableHook::dispatch_mesh_original)>(vtable[79]):nullptr};
    struct Slot {size_t index;void* detour;};
    const Slot slots[]{
        {9,reinterpret_cast<void*>(&ProfileClose)},
        {11,reinterpret_cast<void*>(&VrsClearState)},
        {27,reinterpret_cast<void*>(&VrsExecuteBundle)},
        {51,reinterpret_cast<void*>(&CascadeDiscard)},
        {16,reinterpret_cast<void*>(&CascadeCopyTexture)},
        {17,reinterpret_cast<void*>(&probe_CopyResource)},
        {19,reinterpret_cast<void*>(&RetainedResolve)},
        {48,reinterpret_cast<void*>(&RetainedClearRtv)},
        {46,reinterpret_cast<void*>(&hk_OMSetRenderTargets)},
        {26,reinterpret_cast<void*>(&hk_ResourceBarrier)},
        {15,reinterpret_cast<void*>(&hk_CopyBufferRegion)},
        {59,reinterpret_cast<void*>(&hk_ExecuteIndirect)},
        {14,reinterpret_cast<void*>(&hk_Dispatch)},
        {21,reinterpret_cast<void*>(&hk_RSSetViewports)},
        {22,reinterpret_cast<void*>(&hk_RSSetScissorRects)},
        {10,reinterpret_cast<void*>(&hk_GfxReset)},
        {12,reinterpret_cast<void*>(&hk_DrawInstanced)},
        {13,reinterpret_cast<void*>(&hk_DrawIndexedInstanced)},
        {44,reinterpret_cast<void*>(&hk_IASetVertexBuffers)},
        {29,reinterpret_cast<void*>(&hk_SetComputeRootSignature)},
        {31,reinterpret_cast<void*>(&hk_SetComputeRootDescriptorTable)},
        {25,reinterpret_cast<void*>(&hk_SetPipelineState)},
        {47,reinterpret_cast<void*>(&hk_ClearDepthStencilView)}};
    const Slot probeSlots[]{{16,reinterpret_cast<void*>(&probe_CopyTextureRegion)},{30,reinterpret_cast<void*>(&ProbeGraphicsRoot)},
        {32,reinterpret_cast<void*>(&ProbeGraphicsTable)},{38,reinterpret_cast<void*>(&ProbeGraphicsCbv)},
        {43,reinterpret_cast<void*>(&ProbeIndexBuffer)},
        {55,reinterpret_cast<void*>(&ProbePredication)},{28,reinterpret_cast<void*>(&ProbeHeaps)},
        {20,reinterpret_cast<void*>(&ProbeTopology)},{24,reinterpret_cast<void*>(&ProbeStencil)},
        {23,reinterpret_cast<void*>(&ProbeBlendFactor)}};
    constexpr size_t firstSlot=9;const size_t lastSlot=same7?80:same6?79:sameList5?78:same4?76:same2?66:same1?64:59;
    const size_t bytes=(lastSlot-firstSlot+1)*sizeof(void*);
    DWORD oldProtection{};
    if(!VirtualProtect(vtable+firstSlot,bytes,PAGE_READWRITE,&oldProtection)) {
        ReadObservationUnavailable();
        log("[mirror] cannot make command-list vtable writable: %p",vtable);return;
    }
    g_command_list_vtable_hooks[count]=entry;
    g_command_list_vtable_hook_count.store(count+1,std::memory_order_release);
    for(const auto& slot:slots)
        InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+slot.index),slot.detour);
    if(probeEnabled)for(const auto& slot:probeSlots)
        InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+slot.index),slot.detour);
    // The lens source resolve needs CopyTextureRegion either way; the probe detour calls it too.
    if(!probeEnabled)InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+16),reinterpret_cast<void*>(&hk_CopyTextureRegion));
    if(probeEnabled)CyberpunkVR_NativeStereoBindingHooks.fetch_add(1,std::memory_order_release);
    if(same1)InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+64),reinterpret_cast<void*>(&RetainedResolveRegion));
    if(same4)InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+68),reinterpret_cast<void*>(&RetainedBeginRenderPass));
    if(same4)InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+69),reinterpret_cast<void*>(&RetainedEndRenderPass));
    if(same4 && !probeEnabled)InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+76),reinterpret_cast<void*>(&CensusRayDispatch));
    if(probeEnabled){
        if(same4){const Slot opaque[]{
            {70,reinterpret_cast<void*>(&ObserveMetaCommand<true>)},{71,reinterpret_cast<void*>(&ObserveMetaCommand<false>)},
            {72,reinterpret_cast<void*>(&ObserveRayBuild)},{73,reinterpret_cast<void*>(&ObserveRayEmit)},
            {74,reinterpret_cast<void*>(&ObserveRayCopy)},{76,reinterpret_cast<void*>(&ObserveRayDispatch)}};
            for(const auto& slot:opaque)InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+slot.index),slot.detour);
        }
        if(same6)InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+79),reinterpret_cast<void*>(&ObserveMeshDispatch));
        const Slot observers[]{{18,reinterpret_cast<void*>(&ObserveCopyTiles)},{45,reinterpret_cast<void*>(&ObserveStreamOutput)},
            {54,reinterpret_cast<void*>(&ObserveQueryResolve)},
            {49,reinterpret_cast<void*>(&ObserveClearUavUint)},{50,reinterpret_cast<void*>(&ObserveClearUavFloat)}};
        for(const auto& slot:observers)InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+slot.index),slot.detour);
        if(same1){
            InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+60),reinterpret_cast<void*>(&ObserveAtomicCopy<false>));
            InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+61),reinterpret_cast<void*>(&ObserveAtomicCopy<true>));
        }
        if(same2)InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+66),reinterpret_cast<void*>(&ObserveImmediateWrite));
        if(same7)InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+80),reinterpret_cast<void*>(&ObserveEnhancedBarrier));
    }
    if(sameList5){
        InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+77),reinterpret_cast<void*>(&ProbeShadingRate));
        InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+78),reinterpret_cast<void*>(&ProbeShadingImage));
    }
    DWORD ignored{};
    VirtualProtect(vtable+firstSlot,bytes,oldProtection,&ignored);
    log("[mirror] command-list hooked list=%p vt=%p om=%p rb=%p cr=%p",
        command_list,vtable,(void*)entry.original,(void*)entry.barrier_original,(void*)entry.copyres);
}

// ID3D12Device slots 27 and 29. Only to answer "which resource owns this GPU address" -- a
// vertex-buffer view carries an address and nothing else, and D3D12 offers no way back.
using PFN_CreateCommittedResource = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, const D3D12_HEAP_PROPERTIES*, D3D12_HEAP_FLAGS,
    const D3D12_RESOURCE_DESC*, D3D12_RESOURCE_STATES, const D3D12_CLEAR_VALUE*, REFIID, void**);
using PFN_CreatePlacedResource = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, ID3D12Heap*, UINT64, const D3D12_RESOURCE_DESC*,
    D3D12_RESOURCE_STATES, const D3D12_CLEAR_VALUE*, REFIID, void**);
using PFN_CreateCommittedResource1 = HRESULT (STDMETHODCALLTYPE*)(ID3D12Device4*,const D3D12_HEAP_PROPERTIES*,D3D12_HEAP_FLAGS,
    const D3D12_RESOURCE_DESC*,D3D12_RESOURCE_STATES,const D3D12_CLEAR_VALUE*,ID3D12ProtectedResourceSession*,REFIID,void**);
static PFN_CreateCommittedResource g_orig_CreateCommitted = nullptr;
static PFN_CreateCommittedResource1 g_orig_CreateCommitted1 = nullptr;
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

static void NoteCommittedResource(ID3D12Resource* resource,const D3D12_RESOURCE_DESC* desc,D3D12_RESOURCE_STATES state){
    cvr::cascades::ResourceCreated(resource,nullptr,0,0);
    cvr::stereo::constants::ObserveResourceApi(resource);
    cvr::stereo::constants::RegisteredResourceCreated(resource,false,nullptr,0,0,state);
    cvr::stereo::texture_read::ResourceCreated(resource,state,false);
    cvr::stereo::buffer_read::ResourceCreated(resource,state,false);
    if(cvr::stereo::native_probe::ObserveBindings() && !cvr::stereo::probe::internalCommands)cvr::stereo::geometry::BufferCreated(resource,state);
    buf_note_created(resource,desc);
    if(desc)cvr::diagnostics::ObserveResourceCreation(resource,*desc);
}
static HRESULT STDMETHODCALLTYPE Hook_CreateCommittedResource(
        ID3D12Device* self, const D3D12_HEAP_PROPERTIES* hp, D3D12_HEAP_FLAGS hf,
        const D3D12_RESOURCE_DESC* d, D3D12_RESOURCE_STATES st,
        const D3D12_CLEAR_VALUE* cv, REFIID riid, void** out) {
    HRESULT hr = g_orig_CreateCommitted(self, hp, hf, d, st, cv, riid, out);
    if (SUCCEEDED(hr) && out && *out) {
        NoteCommittedResource(static_cast<ID3D12Resource*>(*out),d,st);
    }
    return hr;
}
static HRESULT STDMETHODCALLTYPE Hook_CreateCommittedResource1(ID3D12Device4* self,const D3D12_HEAP_PROPERTIES* heap,D3D12_HEAP_FLAGS flags,
    const D3D12_RESOURCE_DESC* desc,D3D12_RESOURCE_STATES state,const D3D12_CLEAR_VALUE* clear,ID3D12ProtectedResourceSession* session,REFIID iid,void** out){
    const auto result=g_orig_CreateCommitted1(self,heap,flags,desc,state,clear,session,iid,out);
    if(SUCCEEDED(result) && out && *out)NoteCommittedResource(static_cast<ID3D12Resource*>(*out),desc,state);
    return result;
}

static HRESULT STDMETHODCALLTYPE Hook_CreatePlacedResource(
        ID3D12Device* self, ID3D12Heap* heap, UINT64 off, const D3D12_RESOURCE_DESC* d,
        D3D12_RESOURCE_STATES st, const D3D12_CLEAR_VALUE* cv, REFIID riid, void** out) {
    HRESULT hr = g_orig_CreatePlaced(self, heap, off, d, st, cv, riid, out);
    if (SUCCEEDED(hr) && out && *out) {
        const auto allocationBytes=d?self->GetResourceAllocationInfo(0,1,d).SizeInBytes:0;
        if(d)cvr::cascades::ResourceCreated(static_cast<ID3D12Resource*>(*out),heap,off,allocationBytes);
        cvr::stereo::constants::ObserveResourceApi(static_cast<ID3D12Resource*>(*out));
        cvr::stereo::constants::RegisteredResourceCreated(static_cast<ID3D12Resource*>(*out),true,heap,off,allocationBytes,st);
        cvr::stereo::texture_read::ResourceCreated(static_cast<ID3D12Resource*>(*out),st,true,heap,off,allocationBytes);
        cvr::stereo::buffer_read::ResourceCreated(static_cast<ID3D12Resource*>(*out),st,true);
        if(cvr::stereo::native_probe::ObserveBindings() && !cvr::stereo::probe::internalCommands)cvr::stereo::geometry::BufferCreated(static_cast<ID3D12Resource*>(*out),st);
        buf_note_created(*out, d);
        if (d) cvr::diagnostics::ObserveResourceCreation(static_cast<ID3D12Resource*>(*out), *d);
    }
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
        cvr::stereo::families::RootCreated(static_cast<ID3D12RootSignature*>(*out),nodeMask,blob,len);
        cvr::stereo::native_probe::RootCreated(static_cast<ID3D12RootSignature*>(*out),blob,len);
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
                const auto& rg = p.DescriptorTable.pDescriptorRanges[k];
                const char* t = rg.RangeType == D3D12_DESCRIPTOR_RANGE_TYPE_CBV ? "b"
                              : rg.RangeType == D3D12_DESCRIPTOR_RANGE_TYPE_SRV ? "t"
                              : rg.RangeType == D3D12_DESCRIPTOR_RANGE_TYPE_UAV ? "u" : "s";
                u += snprintf(r + u, sizeof(r) - u, "%s%u+%u/sp%u ", t,
                              rg.BaseShaderRegister, rg.NumDescriptors, rg.RegisterSpace);
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
// on the other pipeline failing to build. Game 2.5: B99868EB53FB5975 (2.31: 9228439BF72D91DB), the
// vertex shader of the same two technique records as the pixel shader (sight_reflex_vs.hlsl says
// what changed in it).
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_SightVsHash = 0xB99868EB53FB5975ull;
static std::vector<uint8_t> g_sight_blob;      // pixel
static std::vector<uint8_t> g_sight_vs_blob;   // vertex
static std::atomic<int> g_sight_blob_state{0};   // 0 untried, 1 loaded, -1 missing

// MAGNIFIED SCOPES KEEP THE RETICLE ON THE GLASS. The collimator build sizes its reticle by angle
// (SIGHT_RETICLE_RELIEF), which is right for a reflex window and wrong for a scope: a scope's
// reticle is authored to fill its eyepiece, and at the angular size it shrank to a third at 5 cm.
// The reticle panels of both kinds are the game's own materials drawn by this one pipeline, so the
// only thing that tells them apart is the optical path the optics mod chose for the weapon in hand
// (SetVRPhysicalOpticsPath). Every sight pipeline gets a twin built with the scope variant of the
// pixel shader (CyberpunkVR_SightPsScope.dxil); SetPipelineState binds the twin while the path is
// 'shader'. Missing file = no twins, collimator mapping for every sight as before.
// kSightVariants holds one file today; each original PSO owns its path variants.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_OpticsPath = 0;   // 0 none 1 collimator 2 shader 3 camera 4 native
extern "C" __declspec(dllexport) int32_t CyberpunkVR_SightScopeTwin = 1;   // 0 = always the collimator build
extern "C" __declspec(dllexport) int32_t CyberpunkVR_SightTwinForce = -1;  // diagnostics: -1 by path, 0 collimator, 1 scope
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_SightScopeTwins = 0;
static constexpr int kSightVariants = 1;
static constexpr const char* kSightVariantFiles[kSightVariants]={"CyberpunkVR_SightPsScope.dxil"};
static std::vector<uint8_t> g_sight_variant_blobs[kSightVariants];
static cvr::render::PipelineVariantIndex<kSightVariants> g_sight_twins{
    GUID{0x93e581cd,0x1498,0x454d,{0x8b,0x9c,0x73,0x6d,0xf4,0x09,0x31,0x7b}}};

static void sight_twin_add(ID3D12PipelineState* sight,ID3D12PipelineState* const (&variants)[kSightVariants],ID3D12PipelineState* mask) {
    bool any=mask!=nullptr;
    for(auto* v:variants)any=any||v;
    if(!any)return;
    const HRESULT hr=g_sight_twins.Store(sight,variants,mask);
    if(hr==S_OK) {
        const auto n=InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_SightScopeTwins));
        log("[pso] sight scope twin %llu: %p -> %p",static_cast<unsigned long long>(n),static_cast<void*>(sight),static_cast<void*>(variants[0]));
    } else if(FAILED(hr))log("[pso] sight variants attachment failed hr=%08X",unsigned(hr));
}

ID3D12PipelineState* sight_scope_twin(ID3D12PipelineState* pso) {
    const int32_t force=CyberpunkVR_SightTwinForce;
    const int32_t path=CyberpunkVR_OpticsPath;
    const int variant=force>=0 ? force-1 : (path==2 ? 0 : -1);
    if(variant<0 || variant>=kSightVariants || !CyberpunkVR_SightScopeTwin)return pso;
    const auto* owner=g_sight_twins.Find(pso);
    auto* twin=owner ? owner->variants[variant].Get() : nullptr;
    return twin ? twin : pso;
}

// The history-mask twin of a sight pipeline while the optics path is 'camera' (the camera
// scope's image surface; sight_mask_ps discards every other sight material), else null.
ID3D12PipelineState* sight_mask_twin(ID3D12PipelineState* pso) {
    if(CyberpunkVR_OpticsPath!=3 || !pso)return nullptr;
    const auto* owner=g_sight_twins.Find(pso);
    return owner ? owner->mask.Get() : nullptr;
}

// Native SetVRPhysicalOpticsPath (Natives/OrientationProvider.cpp).
void SetPhysicalOpticsPath(int32_t path) { CyberpunkVR_OpticsPath=path; }

static bool sight_blob_ready() {
    int st = g_sight_blob_state.load(std::memory_order_acquire);
    if (st) return st > 0;
    static std::mutex mtx;
    std::lock_guard<std::mutex> lk(mtx);
    st = g_sight_blob_state.load(std::memory_order_relaxed);
    if (st) return st > 0;
    char path[MAX_PATH]{};
    if (strcpy_s(path, cvr::paths::DirA(cvr::paths::Folder::Shaders).c_str()) == 0) {
        {
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
                for (int v = 0; v < kSightVariants; ++v) {
                    std::vector<uint8_t>& blob = g_sight_variant_blobs[v];
                    blob.clear();
                    path[dirLen] = 0;
                    strcat_s(path, kSightVariantFiles[v]);
                    HANDLE f = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ, nullptr,
                                           OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
                    if (f != INVALID_HANDLE_VALUE) {
                        LARGE_INTEGER sz{};
                        if (GetFileSizeEx(f, &sz) && sz.QuadPart > 64 && sz.QuadPart < (4 << 20)) {
                            blob.resize(static_cast<size_t>(sz.QuadPart));
                            DWORD got = 0;
                            if (!(ReadFile(f, blob.data(), static_cast<DWORD>(sz.QuadPart),
                                           &got, nullptr) && got == sz.QuadPart &&
                                  memcmp(blob.data(), "DXBC", 4) == 0))
                                blob.clear();
                        }
                        CloseHandle(f);
                    }
                    log("[pso] sight variant %s %s (%zu B)", kSightVariantFiles[v],
                        blob.empty() ? "NOT found -- that path keeps the collimator build" : "loaded",
                        blob.size());
                }
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

// ---- the OUTLINE shader ------------------------------------------------------------------------
//
// VisionMode_Highlight, the compute pass that draws the scanner/focus outline. Replaced so the line
// can be made thicker: its width is nine taps at radius one (the shader's own `c_offsets`), and that
// number exists nowhere else -- not in the scripts, whose whole surface is two colour indices and a
// few flags, and not in any archive, which holds no outline resource at all.
//
// IDENTIFIED BY THE CONTAINER DIGEST, not by an FNV of the bytes: every DXBC/DXIL blob carries its
// own digest at offset 4, and the capture reports it, so the target is known before the game has ever
// run. That is the whole reason this needs no discovery session the way the sight shader did.
// ON, with a self-validating guard rather than a promise: the pipeline is built against the root
// signature the engine has ALREADY bound for this dispatch, so a node that is not the outline pass
// fails creation (its signature does not fit our shader's bindings) and nothing is swapped.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_VisionCsSwap = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVisionCsSwaps = 0;
static const uint8_t kVisionCsDigest[16] = {
    0x78, 0x6E, 0x64, 0xC7, 0x28, 0x8B, 0xCE, 0x14,
    0xF7, 0x70, 0x6B, 0x9A, 0x61, 0xC2, 0x64, 0xFF };
static std::vector<uint8_t> g_vision_cs_blob;
static std::atomic<int> g_vision_cs_state{0};   // 0 untried, 1 loaded, -1 missing

static bool vision_cs_blob_ready() {
    int st = g_vision_cs_state.load(std::memory_order_acquire);
    if (st) return st > 0;
    static std::mutex mtx;
    std::lock_guard<std::mutex> lk(mtx);
    st = g_vision_cs_state.load(std::memory_order_relaxed);
    if (st) return st > 0;
    char path[MAX_PATH]{};
    if (strcpy_s(path, cvr::paths::DirA(cvr::paths::Folder::Shaders).c_str()) == 0) {
        {
            strcat_s(path, "CyberpunkVR_VisionCs.dxil");
            HANDLE f = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ, nullptr,
                                   OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
            if (f != INVALID_HANDLE_VALUE) {
                LARGE_INTEGER sz{};
                if (GetFileSizeEx(f, &sz) && sz.QuadPart > 64 && sz.QuadPart < (4 << 20)) {
                    g_vision_cs_blob.resize(static_cast<size_t>(sz.QuadPart));
                    DWORD got = 0;
                    if (!(ReadFile(f, g_vision_cs_blob.data(), static_cast<DWORD>(sz.QuadPart),
                                   &got, nullptr) && got == sz.QuadPart &&
                          memcmp(g_vision_cs_blob.data(), "DXBC", 4) == 0)) {
                        g_vision_cs_blob.clear();
                    }
                }
                CloseHandle(f);
            }
            if (!g_vision_cs_blob.empty()) {
                g_vision_cs_state.store(1, std::memory_order_release);
                log("[pso] outline CS replacement loaded: %zu B", g_vision_cs_blob.size());
                return true;
            }
            log("[pso] outline CS replacement NOT found (%s) -- original kept", path);
        }
    }
    g_vision_cs_state.store(-1, std::memory_order_release);
    return false;
}

static bool vision_cs_is_target(const D3D12_SHADER_BYTECODE& cs) {
    if (!CyberpunkVR_VisionCsSwap || !cs.pShaderBytecode || cs.BytecodeLength < 20) return false;
    const uint8_t* b = static_cast<const uint8_t*>(cs.pShaderBytecode);
    if (memcmp(b, "DXBC", 4) != 0) return false;
    if (memcmp(b + 4, kVisionCsDigest, sizeof(kVisionCsDigest)) != 0) return false;
    return vision_cs_blob_ready();
}

using PFN_CreateComputePipelineState = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, const D3D12_COMPUTE_PIPELINE_STATE_DESC*, REFIID, void**);
static PFN_CreateComputePipelineState g_orig_CreateComputePso = nullptr;


// The last compute root signature the engine bound anywhere, kept in CommandListCensus.cpp.
// Needed because the thread-local one is null on lists recorded by other threads.
extern std::atomic<ID3D12RootSignature*> g_last_compute_rootsig;

thread_local ID3D12RootSignature* t_current_compute_rootsig = nullptr;
static ID3D12PipelineState* g_vision_cs_pso = nullptr;
static ID3D12RootSignature* g_vision_cs_pso_rs = nullptr;
static std::mutex g_vision_cs_pso_mtx;

// Built once, from OUR shader and the engine's own compute root signature -- the one it has already
// bound for this dispatch, so every descriptor table the engine set still lands where the shader
// expects it. If the signature changes (a different pipeline ran first) the old one is dropped and a
// new pipeline built, which in practice happens never after the first frame.
ID3D12PipelineState* vision_cs_pso_get(ID3D12GraphicsCommandList* list) {
    if (!CyberpunkVR_VisionCsSwap || !list) return nullptr;
    // ONLY THE ONE BOUND FOR THIS DISPATCH. A pipeline built against a DIFFERENT root signature
    // still binds -- the register-to-descriptor mapping simply comes out wrong -- and the
    // failure looks exactly like what was measured: writes to u0/u1 landed (a paint-red variant
    // flooded the frame) while every SRV read came back zero (a probe painting wherever the
    // flags said 'outlined' painted nothing). So the global fallback is not used here: without
    // the real signature we do not substitute at all.
    ID3D12RootSignature* rs = t_current_compute_rootsig;
    if (!rs || !vision_cs_blob_ready()) return nullptr;
    std::lock_guard<std::mutex> lk(g_vision_cs_pso_mtx);
    if (g_vision_cs_pso && g_vision_cs_pso_rs == rs) return g_vision_cs_pso;
    ID3D12Device* dev = nullptr;
    if (FAILED(list->GetDevice(IID_PPV_ARGS(&dev))) || !dev) return nullptr;
    if (g_vision_cs_pso) { g_vision_cs_pso->Release(); g_vision_cs_pso = nullptr; }
    D3D12_COMPUTE_PIPELINE_STATE_DESC d{};
    d.pRootSignature = rs;
    d.CS.pShaderBytecode = g_vision_cs_blob.data();
    d.CS.BytecodeLength = g_vision_cs_blob.size();
    ID3D12PipelineState* pso = nullptr;
    const HRESULT hr = dev->CreateComputePipelineState(&d, IID_PPV_ARGS(&pso));
    dev->Release();
    if (FAILED(hr) || !pso) {
        // Degrade, never refuse: without our pipeline the engine draws its own thin outline.
        static bool s_said = false;
        if (!s_said) {
            s_said = true;
            log("[pso] outline CS pipeline REFUSED hr=0x%08X rs=%p -- engine's own kept",
                static_cast<unsigned>(hr), (void*)rs);
        }
        return nullptr;
    }
    g_vision_cs_pso = pso;
    g_vision_cs_pso_rs = rs;
    log("[pso] outline CS pipeline built against the engine's root signature rs=%p", (void*)rs);
    return g_vision_cs_pso;
}



static HRESULT STDMETHODCALLTYPE Hook_CreateComputePipelineState(
        ID3D12Device* self, const D3D12_COMPUTE_PIPELINE_STATE_DESC* desc,
        REFIID riid, void** out) {
    if (desc && vision_cs_is_target(desc->CS)) {
        D3D12_COMPUTE_PIPELINE_STATE_DESC d = *desc;
        d.CS.pShaderBytecode = g_vision_cs_blob.data();
        d.CS.BytecodeLength  = g_vision_cs_blob.size();
        HRESULT hr2 = g_orig_CreateComputePso(self, &d, riid, out);
        if (SUCCEEDED(hr2)) {
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugVisionCsSwaps);
            log("[pso] outline CS substituted (compute desc) pso=%p", out ? *out : nullptr);
            return hr2;
        }
        // Degrade, never refuse: a rejected replacement leaves the engine's own outline, which is
        // thin but correct. Losing the pipeline entirely would lose the outline for good.
        log("[pso] outline CS substitution REFUSED hr=0x%08X -- original kept",
            static_cast<unsigned>(hr2));
    }
    const HRESULT hr=g_orig_CreateComputePso(self, desc, riid, out);
    if(SUCCEEDED(hr) && out && *out && desc) {
        auto* pso=static_cast<ID3D12PipelineState*>(*out);
        cvr::hidden_area::shaders::PipelineCreated(pso,desc->CS,{});
        // Hidden-area twins: this pipeline with the patched shader, built when first bound.
        if(cvr::hidden_area::shaders::HasPatch(desc->CS))
            cvr::hidden_area::shaders::Register(pso,desc->CS,[self,base=*desc](const D3D12_SHADER_BYTECODE& code)->ID3D12PipelineState* {
                auto twin=base;twin.CS=code;twin.CachedPSO={};ID3D12PipelineState* created{};
                return SUCCEEDED(g_orig_CreateComputePso(self,&twin,IID_PPV_ARGS(&created)))?created:nullptr;
            });
    }
    return hr;
}

// The upscaler history-mask twin of a pipeline (lens_mask_ps / sight_mask_ps): same VS,
// root signature and depth test, one R8_UNORM target, no blending, no depth or stencil writes.
static ID3D12PipelineState* history_mask_twin(ID3D12Device* self,const D3D12_GRAPHICS_PIPELINE_STATE_DESC& base,
                                              const D3D12_SHADER_BYTECODE& ps,const char* what) {
    if(!ps.pShaderBytecode || !ps.BytecodeLength)return nullptr;
    auto twin=base;twin.PS=ps;twin.CachedPSO={};twin.NumRenderTargets=1;
    for(auto& format:twin.RTVFormats)format=DXGI_FORMAT_UNKNOWN;
    twin.RTVFormats[0]=DXGI_FORMAT_R8_UNORM;
    twin.BlendState={};
    for(auto& target:twin.BlendState.RenderTarget)
        target={FALSE,FALSE,D3D12_BLEND_ONE,D3D12_BLEND_ZERO,D3D12_BLEND_OP_ADD,
                D3D12_BLEND_ONE,D3D12_BLEND_ZERO,D3D12_BLEND_OP_ADD,D3D12_LOGIC_OP_NOOP,0};
    twin.BlendState.RenderTarget[0].RenderTargetWriteMask=D3D12_COLOR_WRITE_ENABLE_RED;
    twin.DepthStencilState.DepthWriteMask=D3D12_DEPTH_WRITE_MASK_ZERO;
    twin.DepthStencilState.StencilWriteMask=0;
    ID3D12PipelineState* created=nullptr;
    const auto hr=g_orig_CreateGfxPso(self,&twin,IID_PPV_ARGS(&created));
    if(FAILED(hr)){log("[optics] %s history mask twin rejected hr=%08X",what,unsigned(hr));return nullptr;}
    return created;
}

static HRESULT STDMETHODCALLTYPE Hook_CreateGraphicsPipelineState(
        ID3D12Device* self, const D3D12_GRAPHICS_PIPELINE_STATE_DESC* desc,
        REFIID riid, void** out) {
    if(cvr::stereo::probe::internalCommands)return g_orig_CreateGfxPso(self,desc,riid,out);
    D3D12_SHADER_BYTECODE opticPs{};
    if(desc && cvr::optics::GlassShaderReplacement(desc->PS,opticPs)) {
        auto patched=*desc;patched.PS=opticPs;patched.CachedPSO={};
        const auto hr=g_orig_CreateGfxPso(self,&patched,riid,out);
        if(SUCCEEDED(hr)) {
            if(out && *out){
                pso_ids_record(*out,desc->PS,desc->VS);
                // Twin of the glass draw that writes lens velocity: same VS, root
                // signature and depth test, one R16G16_FLOAT target, no depth or
                // stencil writes.
                ID3D12PipelineState* velocity=nullptr;D3D12_SHADER_BYTECODE velocityPs{};
                if(cvr::optics::LensVelocityShader(velocityPs)) {
                    auto twin=patched;twin.PS=velocityPs;twin.NumRenderTargets=1;
                    for(auto& format:twin.RTVFormats)format=DXGI_FORMAT_UNKNOWN;
                    twin.RTVFormats[0]=DXGI_FORMAT_R16G16_FLOAT;
                    twin.BlendState={};
                    for(auto& target:twin.BlendState.RenderTarget)
                        target={FALSE,FALSE,D3D12_BLEND_ONE,D3D12_BLEND_ZERO,D3D12_BLEND_OP_ADD,
                                D3D12_BLEND_ONE,D3D12_BLEND_ZERO,D3D12_BLEND_OP_ADD,D3D12_LOGIC_OP_NOOP,0};
                    twin.BlendState.RenderTarget[0].RenderTargetWriteMask=D3D12_COLOR_WRITE_ENABLE_RED|D3D12_COLOR_WRITE_ENABLE_GREEN;
                    twin.DepthStencilState.DepthWriteMask=D3D12_DEPTH_WRITE_MASK_ZERO;
                    twin.DepthStencilState.StencilWriteMask=0;
                    const auto twinHr=g_orig_CreateGfxPso(self,&twin,IID_PPV_ARGS(&velocity));
                    if(FAILED(twinHr)){velocity=nullptr;log("[optics] Lens velocity twin rejected hr=%08X",unsigned(twinHr));}
                }
                D3D12_SHADER_BYTECODE maskPs{};
                ID3D12PipelineState* mask=cvr::optics::LensMaskShader(maskPs) ? history_mask_twin(self,patched,maskPs,"Lens") : nullptr;
                cvr::optics::GlassPipelineCreated(static_cast<ID3D12PipelineState*>(*out),velocity,mask);
            }
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugPsoGfx);return hr;
        }
        log("[optics] Glass PS rejected hr=%08X; native pipeline retained",unsigned(hr));
    }
    D3D12_SHADER_BYTECODE markerVs{};
    if(desc && cvr::markers::VertexShaderReplacement(desc->VS,markerVs)) {
        auto patched=*desc;patched.VS=markerVs;patched.CachedPSO={};
        const auto hr=g_orig_CreateGfxPso(self,&patched,riid,out);
        cvr::markers::VertexShaderResult(desc->VS,SUCCEEDED(hr));
        if(SUCCEEDED(hr)) {
            if(out && *out) pso_ids_record(*out,desc->PS,desc->VS);
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugPsoGfx);return hr;
        }
    }
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
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugSightSwaps);
            log("[pso] sight PS substituted (graphics desc) pso=%p", out ? *out : nullptr);
            if (out && *out) {
                ID3D12PipelineState* twins[kSightVariants]{};
                for (int v = 0; v < kSightVariants; ++v) {
                    if (g_sight_variant_blobs[v].empty()) continue;
                    D3D12_GRAPHICS_PIPELINE_STATE_DESC t = d;
                    t.PS.pShaderBytecode = g_sight_variant_blobs[v].data();
                    t.PS.BytecodeLength = g_sight_variant_blobs[v].size();
                    t.CachedPSO = {};
                    const HRESULT ht = g_orig_CreateGfxPso(self, &t, IID_PPV_ARGS(&twins[v]));
                    if (FAILED(ht)) { twins[v] = nullptr; log("[pso] sight twin %d REFUSED hr=0x%08X", v, static_cast<unsigned>(ht)); }
                }
                D3D12_SHADER_BYTECODE maskPs{};
                ID3D12PipelineState* mask = cvr::optics::SightMaskShader(maskPs) ? history_mask_twin(self, d, maskPs, "Sight") : nullptr;
                sight_twin_add(static_cast<ID3D12PipelineState*>(*out), twins, mask);
            }
            if (out && *out) pso_ids_record(*out, desc->PS, desc->VS);  // keep the ORIGINAL id
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugPsoGfx);
            return hr2;
        }
        // A rejected replacement must not cost the game its shader: fall through to the original.
        log("[pso] sight PS substitution REFUSED hr=0x%08X -- keeping the original",
            static_cast<unsigned>(hr2));
    }
    HRESULT hr = g_orig_CreateGfxPso(self, desc, riid, out);
    if (SUCCEEDED(hr) && out && *out && desc) {
        pso_ids_record(*out, desc->PS, desc->VS);
        cvr::hidden_area::shaders::PipelineCreated(static_cast<ID3D12PipelineState*>(*out),{},desc->PS,desc->VS);
        if(cvr::hidden_area::shaders::HasPatch(desc->PS)) {
            auto owned=std::make_shared<cvr::stereo::OwnedGraphicsDesc>();
            if(cvr::stereo::OwnGraphicsDesc(*desc,*owned))
                cvr::hidden_area::shaders::Register(static_cast<ID3D12PipelineState*>(*out),desc->PS,
                    [self,owned](const D3D12_SHADER_BYTECODE& code)->ID3D12PipelineState* {
                        auto twin=owned->desc;twin.PS=code;ID3D12PipelineState* created{};
                        if(const auto vs=cvr::hidden_area::shaders::FixedVertexTwin(twin.VS);vs.pShaderBytecode)twin.VS=vs;
                        return SUCCEEDED(g_orig_CreateGfxPso(self,&twin,IID_PPV_ARGS(&created)))?created:nullptr;
                    });
        } else if(cvr::hidden_area::shaders::HasPatch(desc->VS)) {   // particle facing (asym_particles\)
            auto owned=std::make_shared<cvr::stereo::OwnedGraphicsDesc>();
            if(cvr::stereo::OwnGraphicsDesc(*desc,*owned))
                cvr::hidden_area::shaders::Register(static_cast<ID3D12PipelineState*>(*out),desc->VS,
                    [self,owned](const D3D12_SHADER_BYTECODE& code)->ID3D12PipelineState* {
                        auto twin=owned->desc;twin.VS=code;ID3D12PipelineState* created{};
                        return SUCCEEDED(g_orig_CreateGfxPso(self,&twin,IID_PPV_ARGS(&created)))?created:nullptr;
                    });
        }
        cvr::vrs::GraphicsCreated(static_cast<ID3D12PipelineState*>(*out),*desc);
        cvr::stereo::native_probe::GraphicsCreated(self,*desc,static_cast<ID3D12PipelineState*>(*out));
        cvr::stereo::families::GraphicsCreated(self,*desc,static_cast<ID3D12PipelineState*>(*out));
        CVR_DIAGNOSTIC(++CyberpunkVR_DebugPsoGfx);
    }
    return hr;
}

// The stream form is a tagged blob: {alignas(void*) SUBOBJECT_TYPE, payload} repeated. Walking it
// is the only way to see the shaders, and the walk has to respect the pointer alignment between
// entries or it desynchronises and reads garbage.
// Offsets of the PS and VS payloads inside the tagged stream, SIZE_MAX when absent. Kept in its
// own function on purpose: it needs SEH, and SEH cannot share a frame with objects that unwind.
static void pso_stream_find(const uint8_t* p, size_t len, size_t* psoff, size_t* vsoff, size_t* cacheoff=nullptr) {
    *psoff = SIZE_MAX; *vsoff = SIZE_MAX;
    if(cacheoff) *cacheoff=SIZE_MAX;
    __try {
        cvr::stereo::PipelineStreamOffsets offsets;
        if(cvr::stereo::FindPipelineStreamOffsets({len,const_cast<uint8_t*>(p)},offsets)) {
            *psoff=offsets.ps;*vsoff=offsets.vs;if(cacheoff)*cacheoff=offsets.cached;
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) { *psoff=SIZE_MAX;*vsoff=SIZE_MAX;if(cacheoff)*cacheoff=SIZE_MAX; }
}


static size_t pso_stream_find_cs(const uint8_t* p, size_t len) {
    __try {
        cvr::stereo::PipelineStreamOffsets offsets;
        if(cvr::stereo::FindPipelineStreamOffsets({len,const_cast<uint8_t*>(p)},offsets))return offsets.cs;
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return SIZE_MAX;
}

static HRESULT STDMETHODCALLTYPE Hook_CreatePipelineState(
        ID3D12Device* self, const D3D12_PIPELINE_STATE_STREAM_DESC* desc,
        REFIID riid, void** out) {
    if(cvr::stereo::probe::internalCommands)return g_orig_CreatePso(self,desc,riid,out);
    // Same substitution as the classic path, but the stream is the caller's memory, so it is
    // COPIED first and the copy is patched -- writing into the engine's own description would
    // outlive this call and be visible to whatever else reads it.
    if (desc && desc->pPipelineStateSubobjectStream && desc->SizeInBytes) {
        const uint8_t* csrc = static_cast<const uint8_t*>(desc->pPipelineStateSubobjectStream);
        const size_t csoff = pso_stream_find_cs(csrc, desc->SizeInBytes);
        if (csoff != SIZE_MAX) {
            const auto& cbc = *reinterpret_cast<const D3D12_SHADER_BYTECODE*>(csrc + csoff);
            if (vision_cs_is_target(cbc)) {
                std::vector<uint8_t> copy(csrc, csrc + desc->SizeInBytes);
                auto& ncbc = *reinterpret_cast<D3D12_SHADER_BYTECODE*>(copy.data() + csoff);
                ncbc.pShaderBytecode = g_vision_cs_blob.data();
                ncbc.BytecodeLength = g_vision_cs_blob.size();
                D3D12_PIPELINE_STATE_STREAM_DESC nd = *desc;
                nd.pPipelineStateSubobjectStream = copy.data();
                HRESULT hrc = g_orig_CreatePso(self, &nd, riid, out);
                if (SUCCEEDED(hrc)) {
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugVisionCsSwaps);
                    log("[pso] outline CS substituted (stream desc) pso=%p", out ? *out : nullptr);
                    return hrc;
                }
                log("[pso] outline CS substitution REFUSED hr=0x%08X (stream) -- original kept",
                    static_cast<unsigned>(hrc));
            }
        }
        const uint8_t* src = static_cast<const uint8_t*>(desc->pPipelineStateSubobjectStream);
        size_t psoff = SIZE_MAX, vsoff = SIZE_MAX, cacheoff=SIZE_MAX;
        pso_stream_find(src, desc->SizeInBytes, &psoff, &vsoff,&cacheoff);
        if(psoff!=SIZE_MAX) {
            const auto nativePs=*reinterpret_cast<const D3D12_SHADER_BYTECODE*>(src+psoff);
            D3D12_SHADER_BYTECODE opticPs{};
            if(cvr::optics::GlassShaderReplacement(nativePs,opticPs)) {
                std::vector<uint8_t> copy(src,src+desc->SizeInBytes);
                *reinterpret_cast<D3D12_SHADER_BYTECODE*>(copy.data()+psoff)=opticPs;
                if(cacheoff!=SIZE_MAX)*reinterpret_cast<D3D12_CACHED_PIPELINE_STATE*>(copy.data()+cacheoff)={};
                auto patched=*desc;patched.pPipelineStateSubobjectStream=copy.data();
                const auto hr=g_orig_CreatePso(self,&patched,riid,out);
                if(SUCCEEDED(hr)) {
                    const auto nativeVs=vsoff==SIZE_MAX ? D3D12_SHADER_BYTECODE{} : *reinterpret_cast<const D3D12_SHADER_BYTECODE*>(src+vsoff);
                    if(out && *out){
                        pso_ids_record(*out,nativePs,nativeVs);
                        // The capture shows glass created through the classic
                        // desc; a stream-created glass keeps engine velocity.
                        static std::atomic<bool> logged{};
                        if(!logged.exchange(true))log("[optics] Stream glass pipeline: no lens velocity twin");
                        cvr::optics::GlassPipelineCreated(static_cast<ID3D12PipelineState*>(*out),nullptr,nullptr);
                    }
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugPsoStream);return hr;
                }
                log("[optics] Glass stream PS rejected hr=%08X; native pipeline retained",unsigned(hr));
            }
        }
        if(vsoff!=SIZE_MAX) {
            const auto& originalVs=*reinterpret_cast<const D3D12_SHADER_BYTECODE*>(src+vsoff);
            D3D12_SHADER_BYTECODE markerVs{};
            if(cvr::markers::VertexShaderReplacement(originalVs,markerVs)) {
                std::vector<uint8_t> copy(src,src+desc->SizeInBytes);
                *reinterpret_cast<D3D12_SHADER_BYTECODE*>(copy.data()+vsoff)=markerVs;
                if(cacheoff!=SIZE_MAX) *reinterpret_cast<D3D12_CACHED_PIPELINE_STATE*>(copy.data()+cacheoff)={};
                auto patched=*desc;patched.pPipelineStateSubobjectStream=copy.data();
                const auto hr=g_orig_CreatePso(self,&patched,riid,out);
                cvr::markers::VertexShaderResult(originalVs,SUCCEEDED(hr));
                if(SUCCEEDED(hr)) {
                    const auto ps=psoff!=SIZE_MAX ? *reinterpret_cast<const D3D12_SHADER_BYTECODE*>(src+psoff) : D3D12_SHADER_BYTECODE{};
                    if(out && *out) pso_ids_record(*out,ps,originalVs);
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugPsoStream);return hr;
                }
            }
        }
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
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugSightSwaps);
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugPsoStream);
                    log("[pso] sight PS substituted (stream desc) pso=%p", out ? *out : nullptr);
                    if (out && *out) {
                        ID3D12PipelineState* twins[kSightVariants]{};
                        for (int v = 0; v < kSightVariants; ++v) {
                            if (g_sight_variant_blobs[v].empty()) continue;
                            std::vector<uint8_t> tcopy(copy);
                            auto& tbc = *reinterpret_cast<D3D12_SHADER_BYTECODE*>(tcopy.data() + psoff);
                            tbc.pShaderBytecode = g_sight_variant_blobs[v].data();
                            tbc.BytecodeLength = g_sight_variant_blobs[v].size();
                            if (cacheoff != SIZE_MAX)
                                *reinterpret_cast<D3D12_CACHED_PIPELINE_STATE*>(tcopy.data() + cacheoff) = {};
                            D3D12_PIPELINE_STATE_STREAM_DESC td = *desc;
                            td.pPipelineStateSubobjectStream = tcopy.data();
                            const HRESULT ht = g_orig_CreatePso(self, &td, IID_PPV_ARGS(&twins[v]));
                            if (FAILED(ht)) { twins[v] = nullptr; log("[pso] sight twin %d REFUSED hr=0x%08X (stream)", v, static_cast<unsigned>(ht)); }
                        }
                        // The capture shows sights created through the classic desc; a
                        // stream-created sight draws no history mask.
                        sight_twin_add(static_cast<ID3D12PipelineState*>(*out), twins, nullptr);
                    }
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
        const size_t csoff=pso_stream_find_cs(base,desc->SizeInBytes);
        const auto cs=csoff!=SIZE_MAX?*reinterpret_cast<const D3D12_SHADER_BYTECODE*>(base+csoff):D3D12_SHADER_BYTECODE{};
        cvr::hidden_area::shaders::PipelineCreated(static_cast<ID3D12PipelineState*>(*out),cs,ps,vs);
        // Hidden-area twins: an owned copy of the description (the engine frees its own after this
        // call), rebuilt with the patched compute or pixel shader when first bound.
        // A pipeline whose pixel shader has none may have a particle-facing vertex twin (asym_particles\).
        const bool vertexPatch=csoff==SIZE_MAX && !cvr::hidden_area::shaders::HasPatch(ps) && cvr::hidden_area::shaders::HasPatch(vs);
        const auto& patchedStage=csoff!=SIZE_MAX?cs:vertexPatch?vs:ps;
        if(cvr::hidden_area::shaders::HasPatch(patchedStage)) {
            auto owned=std::make_shared<cvr::stereo::OwnedPipelineStream>();
            if(cvr::stereo::OwnPipelineStream(*desc,*owned)) {
                const size_t stage=csoff!=SIZE_MAX?owned->offsets.cs:vertexPatch?owned->offsets.vs:owned->offsets.ps;
                if(stage!=SIZE_MAX)cvr::hidden_area::shaders::Register(static_cast<ID3D12PipelineState*>(*out),patchedStage,
                    [self,owned,stage,vertexPatch](const D3D12_SHADER_BYTECODE& code)->ID3D12PipelineState* {
                        auto copy=owned->stream;std::memcpy(copy.data()+stage,&code,sizeof(code));
                        // a pixel twin keeps the pipeline's vertex twin (particle facing) too
                        if(!vertexPatch && stage==owned->offsets.ps && owned->offsets.vs!=SIZE_MAX) {
                            D3D12_SHADER_BYTECODE vs{};std::memcpy(&vs,copy.data()+owned->offsets.vs,sizeof(vs));
                            if(const auto twinVs=cvr::hidden_area::shaders::FixedVertexTwin(vs);twinVs.pShaderBytecode)
                                std::memcpy(copy.data()+owned->offsets.vs,&twinVs,sizeof(twinVs));
                        }
                        const D3D12_PIPELINE_STATE_STREAM_DESC twin{copy.size(),copy.data()};ID3D12PipelineState* created{};
                        return SUCCEEDED(g_orig_CreatePso(self,&twin,IID_PPV_ARGS(&created)))?created:nullptr;
                    });
            }
        }
        cvr::stereo::native_probe::StreamCreated(self,*desc,static_cast<ID3D12PipelineState*>(*out));
        cvr::stereo::families::StreamCreated(self,*desc,static_cast<ID3D12PipelineState*>(*out));
        cvr::vrs::StreamCreated(static_cast<ID3D12PipelineState*>(*out),*desc);
        CVR_DIAGNOSTIC(++CyberpunkVR_DebugPsoStream);
    }
    return hr;
}

static HRESULT STDMETHODCALLTYPE Hook_CreateCommandList(
        ID3D12Device* self, UINT node_mask, D3D12_COMMAND_LIST_TYPE type,
        ID3D12CommandAllocator* allocator, ID3D12PipelineState* initial_state,
        REFIID riid, void** out) {
    HRESULT hr = g_orig_CreateCommandList(
        self, node_mask, type, allocator, initial_state, riid, out);
    // COPY lists also mutate resources used by stereo draws. Their Reset/Close
    // hooks bypass graphics-only bookkeeping, but writes and barriers are observed.
    if (SUCCEEDED(hr) && out && *out &&
        (type == D3D12_COMMAND_LIST_TYPE_DIRECT || type == D3D12_COMMAND_LIST_TYPE_COMPUTE || type == D3D12_COMMAND_LIST_TYPE_COPY))
    {
        patch_command_list_vtable(*out);
        auto* list=static_cast<ID3D12GraphicsCommandList*>(*out);
        cvr::stereo::target_handoff::Reset(list);
        cvr::stereo::target_access::Reset(list);
        cvr::stereo::texture_read::Reset(list);
        cvr::stereo::buffer_read::Reset(list);
        cvr::stereo::material_binding::Reset(list);
        if(type==D3D12_COMMAND_LIST_TYPE_COPY){cvr::stereo::constants::ResetRegistered(list);return hr;}
        cvr::stereo::families::Reset(list,initial_state);
        cvr::stereo::constants::Reset(list);
        cvr::stereo::preparation::ResetList(list);
        const auto* entry=command_list_hook_entry(list);
        cvr::vrs::Reset(list,entry && entry->shading_rate_original,initial_state);
        cvr::optics::ListReset(list);
    }
    return hr;
}

void patch_device_descriptor_slot(void* device) {
    if (!device) return;
    bool expected = false;
    if (!g_desc_vtable_patched.compare_exchange_strong(expected, true)) return;
    cvr::stereo::constants::SetReader(cvr::stereo::constants::ReadRegistered,cvr::stereo::constants::ValidateRegistered,
        cvr::stereo::constants::MarkRegisteredRead,cvr::stereo::constants::ResolveRegisteredRead);
    // Engine indirect signatures can predate swapchain creation.
    InstallDepthCaptureHooks(static_cast<ID3D12Device*>(device));
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
        }else ReadObservationUnavailable();
    }else ReadObservationUnavailable();
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
    }else ReadObservationUnavailable();
    // Slot 10 = CreateGraphicsPipelineState. Slot 47 = ID3D12Device2::CreatePipelineState, and
    // it is only touched when the device actually implements ID3D12Device2 -- patching a vtable
    // slot that may not exist is how a hook corrupts the object next to it.
    for (int k = 0; k < 2; ++k) {
        const UINT slot = k ? 29u : 27u;
        DWORD oldp = 0;
        if (!VirtualProtect(&vt[slot], sizeof(void*), PAGE_READWRITE, &oldp)) {
            ReadObservationUnavailable();continue;
        }
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
    {
        Microsoft::WRL::ComPtr<ID3D12Device4> device4;
        if(SUCCEEDED(static_cast<ID3D12Device*>(device)->QueryInterface(IID_PPV_ARGS(&device4)))){
            auto** table=*reinterpret_cast<void***>(device4.Get());DWORD protection{};
            if(VirtualProtect(table+53,sizeof(void*),PAGE_READWRITE,&protection)){
                g_orig_CreateCommitted1=reinterpret_cast<PFN_CreateCommittedResource1>(table[53]);
                InterlockedExchangePointer(reinterpret_cast<void* volatile*>(table+53),reinterpret_cast<void*>(&Hook_CreateCommittedResource1));
                DWORD ignored{};VirtualProtect(table+53,sizeof(void*),protection,&ignored);
                log("[single-pass] CreateCommittedResource1 hooked dev=%p",device4.Get());
            }else ReadObservationUnavailable();
        }
    }
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
    // Slot 11 = CreateComputePipelineState, where the outline shader is built.
    DWORD o11 = 0;
    if (VirtualProtect(&vt[11], sizeof(void*), PAGE_READWRITE, &o11)) {
        g_orig_CreateComputePso = reinterpret_cast<PFN_CreateComputePipelineState>(vt[11]);
        vt[11] = reinterpret_cast<void*>(&Hook_CreateComputePipelineState);
        DWORD junk = 0;
        VirtualProtect(&vt[11], sizeof(void*), o11, &junk);
        log("[pso] CreateComputePipelineState hooked dev=%p", device);
    }
    {
        ID3D12Device2* dev2 = nullptr;
        auto* dev0 = static_cast<ID3D12Device*>(device);
        if (SUCCEEDED(dev0->QueryInterface(IID_PPV_ARGS(&dev2))) && dev2) {
            auto** dev2vt=*reinterpret_cast<void***>(dev2);
            DWORD o47 = 0;
            if (VirtualProtect(&dev2vt[47], sizeof(void*), PAGE_READWRITE, &o47)) {
                g_orig_CreatePso = reinterpret_cast<PFN_CreatePipelineState>(dev2vt[47]);
                dev2vt[47] = reinterpret_cast<void*>(&Hook_CreatePipelineState);
                DWORD junk = 0;
                VirtualProtect(&dev2vt[47], sizeof(void*), o47, &junk);
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
    if(cvr::stereo::descriptors::Enabled())cvr::stereo::descriptors::BorrowedRangeObserverReady(
        g_orig_CreateCBV && g_orig_CreateDescriptorHeap && vt[17]==reinterpret_cast<void*>(&hk_CreateCBV) &&
        vt[14]==reinterpret_cast<void*>(&Hook_CreateDescriptorHeap) && DescriptorMutationHooksReady(static_cast<ID3D12Device*>(device)));
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
