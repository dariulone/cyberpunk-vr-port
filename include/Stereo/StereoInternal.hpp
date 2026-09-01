#pragma once

// ================================================================================================
// The internal interface of the stereo module.
//
// src/Stereo/SyncStereo.cpp was one 13,400-line translation unit because everything in it had
// internal linkage. It is now `namespace cvr::detail`, and this header is what lets its subsystems
// live in separate files: it declares only what actually crosses between them.
//
// THE RULE, so this does not become the monolith with extra steps: a name belongs here when a file
// OTHER than the one defining it uses it. Everything else stays where it is defined. If this header
// ends up listing most of the module, the split has failed.
//
// Declarators are copied from the definitions. g_vrcam_sel_w is a std::atomic<uint32_t>, not the int
// it reads like at the call sites, and writing what it looked like cost a build.
// ================================================================================================

#include <windows.h>
#include <d3d12.h>

// ColorBlit::HudParams crosses the boundary: the mirror asks for the HUD composite parameters and
// the capture path computes them.
#include "Render/ColorBlit.hpp"

#include <atomic>
#include <mutex>
#include <unordered_map>
#include <vector>
#include <cstdint>

// Exported counters the selection writes and the overlay reads live.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_DebugVrcamConfigLoaded;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugVrcamNodeHits;

namespace cvr {
namespace detail {
using CloudsNodeFn = char(__fastcall*)(void*, void*);
using NodeDispatchFn = uint8_t (__fastcall *)(uintptr_t* node, uint8_t* work_context, void* args);
using WaitOnAddressFn = BOOL (WINAPI *)(volatile VOID*, PVOID, SIZE_T, DWORD);
using WakeByAddressAllFn = VOID (WINAPI *)(PVOID);
constexpr uint32_t MAIN_PRESENT_WORK_RVA     = 0x21CDC8;
constexpr uint32_t MAIN_STARTRENDER_WORK_RVA = 0x21AB08;
using DoCullingFn = char(__fastcall*)(void*, void*, void*);
using DrawCompFn = char(__fastcall*)(void*, void*);

// One row of the per-view RenderMask table: a name and the RVA of the engine descriptor that
// carries its bit. The table is defined in SyncStereo.cpp and read by FrameGraph.cpp.
struct RenderMaskEntry { const char* name; uint32_t desc_rva; };
using HandleAssignFn = void*(__fastcall*)(void* dst, void* src);
using ResizeDynTexFn = char (__fastcall*)(void*, void**, uint32_t, uint32_t, int);
using RttViewCreateFn = char (__fastcall*)(__int64, __int64);
constexpr uintptr_t RVA_RENDERER_GLOBAL = 0x3427C00;
using HudViewDataFn = void* (__fastcall*)(void*);
using ViewFeatureCheckFn = uint8_t (__fastcall*)(uintptr_t work_context, uintptr_t required);
struct ProfNode {
    std::atomic<uintptr_t> rva;         // work-fn RVA (0 = empty slot)
    std::atomic<int64_t>   ticks_main;  // INCLUSIVE: this node + the nodes it dispatches
    std::atomic<int64_t>   ticks_vrcam;
    std::atomic<int64_t>   self_main;   // EXCLUSIVE: inclusive minus its child dispatches
    std::atomic<int64_t>   self_vrcam;
    std::atomic<uint32_t>  calls_main;
    std::atomic<uint32_t>  calls_vrcam;
    std::atomic<uint32_t>  ord_main;    // 1-based first-seen dispatch order (0 = never seen)
    std::atomic<uint32_t>  ord_vrcam;
};
struct ProfPass {
    std::atomic<int64_t>  ticks_main;
    std::atomic<int64_t>  ticks_vrcam;
    std::atomic<uint32_t> calls_main;
    std::atomic<uint32_t> calls_vrcam;
};
constexpr uint32_t  DESC_HEAP_SIZE_NEW     = 0x001DC240u;
constexpr uint32_t  DESC_HEAP_SIZE_ORIG    = 0x000DC240u;
using CreateCBVFn = void (STDMETHODCALLTYPE*)(ID3D12Device*,
    const D3D12_CONSTANT_BUFFER_VIEW_DESC*, D3D12_CPU_DESCRIPTOR_HANDLE);
constexpr uint32_t GRADE_CB_MAX   = 512;
constexpr uint32_t GRADE_CB_SLOTS = 8;

// Which snapshot slot a capture belongs to. Shared because the HUD owns the composite but the capture
// path is what notices a bind and files it under a slot.
enum HudSnapSlot { kSnapHud = 0, kSnapBlur, kSnapMainOut, kSnapMainScene, kSnapVision };
struct MappedUpload { ID3D12Resource* res; uint8_t* ptr; uint64_t size; uint64_t va; };
// The descriptor-handle -> dimensions map entry. The HUD asks what a bound target's size is, and the
// answer is recorded by the capture path, so the type is complete here.
struct RtvDimEntry {
    std::atomic<SIZE_T> handle{0};
    uint32_t w = 0, h = 0;
    ID3D12Resource* res = nullptr;
};

// The light-upload destination record. A complete type at the TOP of this namespace, because
// std::array<LightDst, 24> below needs it complete -- a definition appended after its use is how the
// resolver looped five times adding the same line.
struct LightDst { void* res; uint64_t size; uint32_t hits[2]; uint32_t last_bytes; };
constexpr uint32_t CLUSTERED_LIGHTS_CULL_RVA = 0x77CED4;
constexpr uint32_t RENDER_LIGHT_BUFFERS_RVA  = 0x77D308;
using GiNodeFn = char(__fastcall*)(void*, void*);
using SkyWorkFn = void(__fastcall*)(void*, void*);
using CbUploadFn = __int64(__fastcall*)(unsigned int, void*);
constexpr uintptr_t OFF_VIEWSTATE       = 0x4658;

// ---- offsets inside the DLSS / Streamline state, live-verified ---------------------------------
// A table, for the same reason EngineRvas.hpp is one: these are checked as a set against a game
// build, not one at a time.
constexpr uintptr_t DLSS_FLAGSET_OFF   = 0x17D8;     // inner+0x17D0 + (0x45>>6)*8
constexpr uint64_t  DLSS_EVAL_FLAG_BIT = (1ull << 5);// flag 0x45 (bit 5 of that qword)
constexpr uintptr_t DLSS_CACHE_OFF = 0x3C8;   // a1+968 .. +0x3E7 (changed-detection cache)

constexpr uintptr_t DLSS_VP_OFF     = 0x498;   // viewport id inside the DLSS state (a1)
constexpr uintptr_t DLSS_JITTER_OFF = 0x1E0;   // jitterX @ +0x1E0, jitterY @ +0x1E4, live-verified

// The COM vtable signatures the command-list hook record is written in terms of. Multi-line
// `using` declarations, which is why a per-line search for them found nothing.
using PFN_D3D12CreateDevice = HRESULT (WINAPI*)(IUnknown*, D3D_FEATURE_LEVEL,
                                                REFIID, void**);
using PFN_CreateDescriptorHeap = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, const D3D12_DESCRIPTOR_HEAP_DESC*, REFIID, void**);
using PFN_ExecuteCommandLists =
    void (STDMETHODCALLTYPE*)(ID3D12CommandQueue*, UINT, ID3D12CommandList* const*);
using PFN_CreateCommandQueue =
    HRESULT (STDMETHODCALLTYPE*)(ID3D12Device*, const D3D12_COMMAND_QUEUE_DESC*, REFIID, void**);
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
// Slot 47. Hooked to withhold ONE clear: the shadow atlas is one texture written by both views, and the
// second view fills it first, so MAIN's clear is what destroys the copy it could otherwise have reused.
using PFN_ClearDepthStencilView = void (STDMETHODCALLTYPE*)
    (ID3D12GraphicsCommandList*, D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_CLEAR_FLAGS, FLOAT, UINT8,
     UINT, const D3D12_RECT*);
using PFN_Dispatch = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*, UINT, UINT, UINT);
using PFN_ExecuteIndirect = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,
    ID3D12CommandSignature*, UINT, ID3D12Resource*, UINT64, ID3D12Resource*, UINT64);
using PFN_DrawInstanced = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,
    UINT, UINT, UINT, UINT);
using PFN_DrawIndexedInstanced = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,
    UINT, UINT, UINT, INT, UINT);
using PFN_SetPipelineState = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,
    ID3D12PipelineState*);
using PFN_IASetVertexBuffers = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,
    UINT, UINT, const D3D12_VERTEX_BUFFER_VIEW*);
using PFN_CopyTextureRegion = void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,
    const D3D12_TEXTURE_COPY_LOCATION*, UINT, UINT, UINT,
    const D3D12_TEXTURE_COPY_LOCATION*, const D3D12_BOX*);
using PFN_CopyBufferRegion = void (STDMETHODCALLTYPE*)
    (ID3D12GraphicsCommandList*, ID3D12Resource*, UINT64, ID3D12Resource*,
     UINT64, UINT64);
using PFN_RSSetViewports = void (STDMETHODCALLTYPE*)
    (ID3D12GraphicsCommandList*, UINT, const D3D12_VIEWPORT*);
using PFN_RSSetScissorRects = void (STDMETHODCALLTYPE*)
    (ID3D12GraphicsCommandList*, UINT, const D3D12_RECT*);
using PFN_GfxReset = HRESULT (STDMETHODCALLTYPE*)
    (ID3D12GraphicsCommandList*, ID3D12CommandAllocator*, ID3D12PipelineState*);
using PFN_CreateCommittedResource = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, const D3D12_HEAP_PROPERTIES*, D3D12_HEAP_FLAGS,
    const D3D12_RESOURCE_DESC*, D3D12_RESOURCE_STATES, const D3D12_CLEAR_VALUE*, REFIID, void**);
using PFN_CreatePlacedResource = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, ID3D12Heap*, UINT64, const D3D12_RESOURCE_DESC*,
    D3D12_RESOURCE_STATES, const D3D12_CLEAR_VALUE*, REFIID, void**);
using PFN_CreateRootSignature = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, UINT, const void*, SIZE_T, REFIID, void**);
using PFN_CreateGraphicsPipelineState = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, const D3D12_GRAPHICS_PIPELINE_STATE_DESC*, REFIID, void**);
using PFN_CreatePipelineState = HRESULT (STDMETHODCALLTYPE*)(
    ID3D12Device*, const D3D12_PIPELINE_STATE_STREAM_DESC*, REFIID, void**);
using SlConstFn = __int64(__fastcall*)(void*, void*, void*);
using ApplyDlssFn = __int64(__fastcall*)(void*, void*);
using DlssConstFn = __int64(__fastcall*)(void*, unsigned int);
using DlssEvalFn  = void(__fastcall*)(void*, void*, int, int, int, int, int,
                                      int, int, int, int, int, int, int);
using FineMaterializeFn = char(__fastcall*)(void*, __int64*, char, void*);
using PrepareFilterFn = __int64(__fastcall*)(void*, char, uint32_t, uint32_t);
using PrepareSortFn = void(__fastcall*)(void*, void*, uint32_t, void*);
using PrepareStageFn = void(__fastcall*)(void*, void*, void*, uint32_t, uint32_t);
using VisibleAppendFn = char(__fastcall*)(void*, uintptr_t*);
using MainCullCtxInitFn = void*(__fastcall*)(void*, uintptr_t, void*);
using MainCullPrepareFn = __int64(__fastcall*)(void*, void*, void*);
using MainCullTestFn = __int64(__fastcall*)(void*, void*, void*, void*, void*);
using PrepareFinalizeFn = void(__fastcall*)(void*, char, __int64, __int64);
using PrepareGatherFn = void*(__fastcall*)(void*, void*);
using QueryWorkFn = __int64(__fastcall*)(void*, void*);
using VisQueryPrepareFn = __int64(__fastcall*)(void*, void*);
using VisibilityCollectorFn = __int64(__fastcall*)(void*, void*);
constexpr size_t    DLSS_CACHE_SZ  = 32;

// The command-list vtable hook record. In the header because the DLSS path asks which hook a given
// command list belongs to, and the answer is defined elsewhere.
struct CommandListVtableHook {
    void** vtable = nullptr;
    PFN_OMSetRenderTargets original = nullptr;          // slot 46 (hooked)
    PFN_ResourceBarrier    barrier_original = nullptr;  // slot 26 (hooked)
    PFN_ResourceBarrier    barrier_call = nullptr;      // slot 26 raw (for appending)
    PFN_CopyResource       copyres = nullptr;           // slot 17 raw (for appending)
    PFN_CopyTextureRegion  copytex = nullptr;           // slot 16 raw (tile-grid probe)
    PFN_ExecuteIndirect    indirect_original = nullptr; // slot 59 (hooked, see census)
    PFN_DrawInstanced        draw_original = nullptr;      // slot 12 (draw census)
    PFN_DrawIndexedInstanced drawidx_original = nullptr;   // slot 13 (draw census)
    PFN_CopyBufferRegion   cbr_original = nullptr;      // slot 15 (hooked, CB probe)
    PFN_Dispatch           dispatch_original = nullptr; // slot 14 (hooked, node naming)
    PFN_RSSetViewports     viewports_original = nullptr;// slot 21 (hooked, crop fix)
    PFN_RSSetScissorRects  scissor_original = nullptr;  // slot 22 (hooked, crop fix)
    PFN_GfxReset           reset_original = nullptr;    // slot 10 (hooked, phase reset)
    PFN_SetPipelineState   setpso_original = nullptr;   // slot 25 (hooked, PSO probe)
    PFN_IASetVertexBuffers iavb_original = nullptr;     // slot 44 (hooked, sight axis probe)
    PFN_ClearDepthStencilView cleardsv_original = nullptr;  // slot 47 (hooked, cascade atlas reuse)
};

// Function-pointer types the trampoline pointers are declared in terms of. They have to be here
// for the same reason the pointers are: two files name them now.

// ---- types the culling subsystem is named in terms of ------------------------------------------
//
// A struct, a cap and an enum. They are here rather than beside the code that uses them because two
// files now name them: the visibility replay records batches in one and consumes them in the other.
struct ReplayVisibilityBatch {
    uintptr_t tags[32]{};
    uint32_t count = 0;
    uint32_t reserved = 0;
};
constexpr uint32_t CULL_CALLBACK_MAX = 128;
enum : uint32_t { FINE_REUSE_IDLE = 0, FINE_REUSE_CAPTURE = 1, FINE_REUSE_REPLAY = 2 };

// The loaded image base every RVA in this module is taken from. Resolved once at init; a detour
// installed before it is known would land at offset zero.
extern uint8_t* g_exe_base;

// ---- VRCAM identity ----------------------------------------------------------------------------
//
// The four coupled fields the selection resolves TOGETHER, plus the hash every "is this the second
// eye?" test compares against. Resolving one of them alone is how the render target and the
// component came to disagree, which is why they are declared as a group rather than one by one.
extern char g_vrcam_component[96];
extern char g_vrcam_camera[96];
extern std::atomic<uint32_t> g_vrcam_sel_w;
extern std::atomic<uint32_t> g_vrcam_sel_h;
extern std::atomic<uint64_t> g_vrcam_ctx_key;
// True once the launcher resolution has decided the pick, so a later file read cannot undo it.
extern bool g_vrcam_pick_authoritative;

// Resolves all of the above from vrcam.json and the launcher ini. Called at init, and again by the
// watcher when the answer changes.
void load_vrcam_selection();

// Its own thread: adopts the camera CET actually enabled, and re-reads the launcher pick, which
// arrives after our init-time read.
void vrcam_active_watcher();

// ---- the CPU profiler: accumulators drained once per frame from the Present hook -------------
extern "C" __declspec(dllexport) double   CyberpunkVR_ProfFrameMs;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_ProfEnable;
extern LARGE_INTEGER g_qpc_freq;
extern double        g_qpc_to_ms;
extern std::atomic<int64_t>  g_prof_build_main_ticks;
extern std::atomic<int64_t>  g_prof_build_vrcam_ticks;
extern std::atomic<int64_t>  g_prof_disp_main_ticks;
extern std::atomic<int64_t>  g_prof_disp_vrcam_ticks;
extern std::atomic<int64_t>  g_prof_window_t0;
extern std::atomic<uint64_t> g_prof_build_main_calls;
extern std::atomic<uint64_t> g_prof_build_vrcam_calls;
extern std::atomic<uint64_t> g_prof_disp_main_nodes;
extern std::atomic<uint64_t> g_prof_disp_vrcam_nodes;
extern std::atomic<uint64_t> g_prof_frames;
extern std::atomic<uint64_t> g_prof_top_main;
extern std::atomic<uint64_t> g_prof_top_vrcam;
int64_t prof_now();

// ---- shared with the culling / visibility / prepare subsystem ---------------------------------
bool is_main_view(const void* view);
extern "C" __declspec(dllexport) float    CyberpunkVR_LodThreshValue;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CullCallbackProfileEnable;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CullReuseMode;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugLodThreshSeenMainBits;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugLodThreshSeenVrcamBits;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_LodThreshApplyMask;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_LodThreshOverrideEnable;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_MaterializeProfileEnable;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_OcclusionGateForce;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullCallbackCallsMain[CULL_CALLBACK_MAX];
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullCallbackCallsVrcam[CULL_CALLBACK_MAX];
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullCallbackDescMain[CULL_CALLBACK_MAX];
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullCallbackDescVrcam[CULL_CALLBACK_MAX];
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullCallbackMethodRva[CULL_CALLBACK_MAX];
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullCallbackTicksMain[CULL_CALLBACK_MAX];
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullCallbackTicksVrcam[CULL_CALLBACK_MAX];
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFineCandidateCaptures;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFineCandidateFallbacks;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFineCandidateReplays;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFineDrawableIdsCaptured;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFineDrawableIdsReplayed;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLodThreshHitsMain;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLodThreshHitsOther;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLodThreshHitsVrcam;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainCullPrepareSkips;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeDrawableAppendsMain;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeDrawableAppendsVrcam;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeFineCallsMain;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeFineCallsVrcam;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeFineRangesMain;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeFineRangesVrcam;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVisibilityCandidatesCaptured;
extern FineMaterializeFn g_orig_fine_materialize;
extern MainCullCtxInitFn g_main_cull_ctx_init;
extern MainCullCtxInitFn g_orig_gather_ctx_init;
extern MainCullPrepareFn g_orig_main_cull_prepare;
extern PrepareFilterFn g_orig_prepare_filter;
extern PrepareFinalizeFn g_orig_prepare_finalize;
extern PrepareGatherFn g_orig_prepare_gather;
extern PrepareSortFn g_orig_prepare_sort_a;
extern PrepareSortFn g_orig_prepare_sort_b;
extern PrepareSortFn g_orig_prepare_sort_c;
extern PrepareSortFn g_orig_prepare_sort_final;
extern PrepareStageFn g_orig_prepare_stage;
extern QueryWorkFn g_orig_querywork;
extern VisQueryPrepareFn g_orig_visquery_prepare;
extern VisibleAppendFn g_orig_visible_append;
extern std::atomic<uint32_t> g_fine_reuse_phase;
extern std::mutex g_fine_visibility_mutex;
extern std::unordered_map<uintptr_t, std::vector<uintptr_t>> g_fine_visible_ids;
// Raised inside the RTT preparer's call for OUR view, by camera-name hash. The narrowest scope
// in which this view's parameters are still being decided.
extern thread_local bool t_rtt_prepare_ours;
void view_params_report();
void complend_report();
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_RunViewParams;
extern thread_local bool t_capture_fine_ids;
extern thread_local bool t_capture_vrcam_visibility;
extern thread_local std::vector<ReplayVisibilityBatch> t_vrcam_visibility_batches;
extern thread_local std::vector<uintptr_t> t_fine_ids;
extern thread_local uintptr_t t_materialize_output_key;
uint64_t prepare_mix64(uint64_t value);
void lod_thresh_report();
void materialize_prof_add(uint64_t& main_value, uint64_t& vrcam_value, uint64_t value);
void materialize_range_ctx_observe(uint64_t key_hash);
void materialize_range_observe(uint64_t key_hash);

extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLocalCtxZeroHits;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVisibilityBatchCaptures;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVisibilityBatchReplays;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVisibilityCandidatesReplayed;
extern MainCullTestFn g_orig_main_cull_test;
extern VisibilityCollectorFn g_orig_visibility_collector;
extern std::atomic<uint64_t> g_vrcam_visibility_generation;
extern std::mutex g_visibility_batches_mutex;
extern std::vector<ReplayVisibilityBatch> g_vrcam_visibility_batches;

// ---- shared with the DLSS / Streamline subsystem ----------------------------------------------
const CommandListVtableHook* command_list_hook_entry( ID3D12GraphicsCommandList* command_list);
extern "C" __declspec(dllexport) float CyberpunkVR_DebugMainCamFov;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugMainProjYY;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugVrcamWantFov;
// MAIN's live ADS magnification, recovered from its own forward projection
// (ads = projYY * tan(baseFov/2)): 1.0 at hip, ~1.3-1.5 for ordinary sights, 4.25 for a
// sniper scope. Renamed from CyberpunkVR_DebugVrcamZoomFactor, which described neither the
// view it belongs to nor the fact that three separate consumers aim with it (dabinn,
// TofuExpress 4f676e33).
extern "C" __declspec(dllexport) float CyberpunkVR_MainAdsZoomFactor;
extern "C" __declspec(dllexport) float CyberpunkVR_VrcamFovDeg;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_VrcamDlss;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_VrcamDlssViewport;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_VrcamDlssZeroJitter;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMainAaMode;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMainBuildModeF90;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugVrcamAaMode;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugVrcamBuildModeF90;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_ForceVrcamCam;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamCamFields;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_StreamlineHistoryFix;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamComputeResolve;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamDlssScale;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugForceCamHits;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainCtx;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainCtxBinds;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugSlHistoryHits;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamApplyDlssHits;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamDlssConstHits;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamDlssEvalHits;
extern ApplyDlssFn g_orig_applydlss;
extern DlssConstFn g_orig_dlss_const;
extern DlssEvalFn  g_orig_dlss_eval;
extern SlConstFn g_orig_sl_const;
extern bool    g_vrcam_dlss_cache_valid;
extern float g_ads_factor;
extern float g_main_cam_far;
extern float g_main_cam_fov;
extern float g_main_cam_near;
extern float g_main_cam_zoom;
extern float g_main_proj_yy;
extern float g_vrcam_base_fov;
extern std::atomic<uintptr_t> g_main_view_ctx;
extern std::atomic<uintptr_t> g_main_view_obj;
extern std::atomic<uintptr_t> g_vrcam_comp;
extern uint8_t g_vrcam_dlss_cache[DLSS_CACHE_SZ];
extern thread_local bool t_vrcam_dlss_post;
extern thread_local bool t_vrcam_sl_active;
// Which view an engine object belongs to. Defined with the camera writer.
uintptr_t sl_view_obj(void* work_context);

// REPAIRED: this declaration was on the same line as sl_view_obj and three comment fragments, so it
// sat AFTER a `//` and was switched off. Same generator fault as the nine in Camera/CameraState.hpp.
//
// True for MAIN view ctx. The aspect test survives only as a bootstrap for the first frames, before
// the cache is warm; once it is, it is never consulted again -- which is what keeps this correct when
// MAIN goes square.
bool is_main_view(const void* view);

using FlagComputeFn = __int64(__fastcall*)(void*, __int64, __int64, __int64);
extern FlagComputeFn g_orig_flag_compute;
extern __int64 __fastcall Detour_FlagCompute(void* a1, __int64 a2, __int64 a3, __int64 a4);
extern volatile int32_t g_vrcam_dlss_ow;
extern volatile int32_t g_vrcam_dlss_oh;
extern volatile int32_t g_vrcam_dlss_rw;
extern volatile int32_t g_vrcam_dlss_rh;
extern thread_local ID3D12GraphicsCommandList* t_hud_rt_list;
extern thread_local ID3D12Resource* t_hud_rt_bound;
extern thread_local UINT t_cur_rt_h;
extern thread_local UINT t_cur_rt_w;

// The viewport hook lives with the DLSS band that upscales through it; the vtable patch that
// installs it does not.
void STDMETHODCALLTYPE hk_RSSetViewports(ID3D12GraphicsCommandList*, UINT, const D3D12_VIEWPORT*);

void STDMETHODCALLTYPE hk_RSSetScissorRects(ID3D12GraphicsCommandList*, UINT, const D3D12_RECT*);
HRESULT STDMETHODCALLTYPE hk_GfxReset(ID3D12GraphicsCommandList*, ID3D12CommandAllocator*, ID3D12PipelineState*);

__int64 grade_up_mirror_call(unsigned int size, void* src);
bool grade_up_capture(void* src, int v);
bool grade_up_is_target(unsigned int size, void* src);
extern "C" __declspec(dllexport) int32_t CyberpunkVR_GradeCbProbe;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_GradeUpProbe;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_GradeMirrorMask;
extern CbUploadFn g_orig_cb_upload;
extern bool     g_gcu_seen[2];
extern thread_local bool t_vrcam_node_active;
// The same question with three answers instead of two: 0 = MAIN, 1 = the second eye, -1 = neither.
// Reading the flag above as "not vrcam, therefore MAIN" put reflection-probe cubemap faces into
// MAIN's column and made five separate census readings mixtures. Any per-view comparison must
// ignore -1.
extern thread_local int32_t t_view_side;
// Which shadow cascade the pass currently executing is for, published by Detour_CascadeNode so a probe on
// the constant uploads can key on it. -1 outside the cascade pass. See the note at that detour: keying too
// coarsely is what invalidated three earlier measurements.
extern thread_local int32_t t_cascade_idx;
extern thread_local int      t_grade_cb_view;
extern thread_local uint32_t t_grade_cb_idx;
void grade_cb_commit(int kind, uint32_t n);
void grade_cb_report();
void grade_up_report();

bool cloud_cb_raw_copy(void* dst, const void* src, size_t n);
// The composition state, lent around ONE node call. Returns the viewData written, or 0.
uintptr_t comp_lend_install(void* work_context, uint64_t saved[2]);
void comp_lend_restore(uintptr_t vd, const uint64_t saved[2]);
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CompLendScoped;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DistantReuseMode;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_GiReuseMode;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_LocalShadowReuseMode;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_SkyReuseMode;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugGiSkipHits;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugSkyMainHits;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugSkySkipHits;
extern GiNodeFn g_orig_gi_node;
extern SkyWorkFn g_orig_sky_work;
void __fastcall Detour_SkyWork(void* a1, void* a2);

void viewdata_fill_from_wc(void* wc);

ColorBlit::HudParams hud_composite_params();
bool d12_mirror_ensure(const D3D12_RESOURCE_DESC& src);
bool mirror_get_resource_desc(ID3D12Resource* resource, D3D12_RESOURCE_DESC* desc);
double luma_probe_collect(uint32_t idx);
extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetFrameConstantBuffer();
extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetHudBlurTexture();
extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetHudConstantBuffer();
extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetHudExposureBuffer();
extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetHudTexture();
extern "C" __declspec(dllexport) ID3D12Resource* CyberpunkVR_GetVisionTexture();
extern "C" __declspec(dllexport) float CyberpunkVR_VisionOffX;
extern "C" __declspec(dllexport) float CyberpunkVR_VisionOffY;
extern "C" __declspec(dllexport) int   CyberpunkVR_VisionFit;
extern "C" __declspec(dllexport) int CyberpunkVR_VisionDebug;
extern "C" __declspec(dllexport) int CyberpunkVR_VisionToSecondEye;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_LumaProbe;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_MirrorTrackState;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_StableCopy;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_VrcamOwnTarget;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_VisionDump;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMirrorSrcState;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_MirrorOutput;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorBarrierHits;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVisionOverlays;
extern "C" __declspec(dllexport) void CyberpunkVR_NoteHudCompositeInputs( const void* hud, const void* blur, const void* expo, const void* frameCb, const void* hudCb);
extern "C" float    CyberpunkVR_BarrelDotNdcX2;
extern "C" float    CyberpunkVR_BarrelDotNdcY;
extern "C" float    CyberpunkVR_BarrelDotRadiusPx;
extern "C" int      CyberpunkVR_BarrelDotSecondEye;
extern "C" unsigned long long CyberpunkVR_BarrelDotTick;
extern D3D12_CPU_DESCRIPTOR_HANDLE g_own_rtv;
extern DXGI_FORMAT           g_d12_fmt;
extern ID3D12CommandAllocator*    g_d12_copy_alloc[4];
extern ID3D12DescriptorHeap*       g_own_rtv_heap;
extern ID3D12Device* g_game_device;
extern ID3D12Fence*          g_d12_fence;
extern ID3D12GraphicsCommandList* g_d12_copy_list;
extern ID3D12Resource*             g_own_target;
extern ID3D12Resource*       g_d12_mtex;
extern ID3D12Resource*     g_stable_tex;
extern ID3D12Resource* g_visdump_rb;
extern UINT                  g_d12_w;
extern UINT                  g_d12_h;
extern bool                     g_ti_valid[4];
extern bool                  g_d12_mtex_is_rt;
extern bool            g_cb_valid[4];
extern bool            g_luma_valid[4];
extern int      g_visdump_slot;
extern std::atomic<ID3D12GraphicsCommandList*> g_mirror_pending_list;
extern std::atomic<ID3D12Resource*> g_captured_vrcam_res;
extern std::atomic<bool>   g_stable_fresh;
extern std::atomic<bool> g_mirror_copy_armed;
extern std::atomic<uint64_t> g_mirror_armed_serial;
extern std::atomic<uint64_t> g_mirror_vrcam_serial;
extern std::mutex                  g_own_target_mtx;
extern std::mutex                 g_d12_copy_mtx;
extern std::mutex            g_d12_mtx;
extern std::mutex          g_stable_mtx;
extern uint32_t                   g_d12_copy_frame;
extern uint32_t g_visdump_w;
extern uint32_t g_visdump_h;
extern uint32_t g_visdump_pitch;
extern uint64_t                   g_d12_copy_slot_fence[4];
extern uint8_t*                 g_ti_map[4];
extern uint8_t*        g_cb_map[4];
extern uint8_t*        g_luma_map[4];
extern void*    g_visdump_map;
void cb_probe_collect(uint32_t idx, double L);
void d12_submit_mirror_copy(ID3D12CommandQueue* queue);
void ti_probe_collect(uint32_t idx, double L);
void vision_dump_write();

bool cb_probe_ensure(uint32_t idx);
bool luma_probe_ensure(uint32_t idx);
bool ti_probe_ensure(uint32_t idx);
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugFinNatural;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMirrorLastHr;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMirrorState;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_MirrorTestPattern;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorFrames;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorReadyFence;
extern "C" unsigned long long CyberpunkVR_DebugBarrelDotDraws;
using CreateRTVFn = void (STDMETHODCALLTYPE*)(ID3D12Device*, ID3D12Resource*,
    const D3D12_RENDER_TARGET_VIEW_DESC*, D3D12_CPU_DESCRIPTOR_HANDLE);
extern CreateRTVFn g_orig_CreateRTV;
extern ID3D12Resource*          g_ti_rb[4];
extern ID3D12Resource*          g_ti_src[4][24];
extern ID3D12Resource* g_cb_rb[4];
extern ID3D12Resource* g_luma_rb[4];
extern PFN_ExecuteCommandLists g_orig_ExecuteCommandLists;
// The tonemap input-capture record. A complete type rather than a forward declaration, because the
// mirror's luma probe indexes the array and reads its members.
struct TmInCap {
    std::atomic<ID3D12Resource*> res{nullptr};
    std::atomic<uint32_t>        state{0};
    std::atomic<uint32_t>        fmt{0};
};
extern TmInCap                  g_tm_in[24];
extern std::atomic<ID3D12Resource*> g_cb_last_res;
extern std::atomic<ID3D12Resource*> g_cb_res;
extern std::atomic<ID3D12Resource*> g_expo_main;
extern std::atomic<ID3D12Resource*> g_expo_vrcam;
extern std::atomic<bool>            g_cb_reset_pending;
extern std::atomic<uint32_t>    g_tm_in_n;
extern std::atomic<uint32_t>    g_tm_in_rva[24];
extern std::atomic<uint64_t>        g_cb_off;
extern uint32_t                 g_ti_count[4];
extern uint32_t                 g_ti_fmt[4][24];
extern uint32_t                 g_ti_tag[4][24];
extern uint32_t        g_luma_finidx[4];
extern uint32_t        g_luma_frame[4];
extern uint32_t        g_luma_parity[4];
void d12_append_mirror_copy(const CommandListVtableHook* e, ID3D12GraphicsCommandList* list, ID3D12Resource* dtex, D3D12_RESOURCE_STATES after);
void d12_present_thread();

bool hud_cb_content_matches(const uint8_t* base, UINT64 off, float w, float h, float* curvature_out);
const uint8_t* upload_cpu_for_va(uint64_t va, uint64_t need);
const uint8_t* upload_map_read(ID3D12Resource* res);
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_LightCensus;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCbCaptures;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFrameCb;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudCb;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLightBytesMain;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLightBytesVrcam;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLightUploadsMain;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLightUploadsVrcam;
extern D3D12_RESOURCE_DESC g_hud_snap_desc;
extern std::array<LightDst, 24> g_light_dsts;
extern std::atomic<ID3D12Resource*> g_frame_cb;
extern std::atomic<ID3D12Resource*> g_hud_cb;
extern std::atomic<bool> g_hud_cb_from_ring;
extern std::mutex g_hud_snap_mtx;
extern thread_local UINT t_last_disp[3];
extern thread_local bool t_2rt_cb_armed;
extern thread_local bool t_in_vrcam_2rt;
extern thread_local uintptr_t t_current_node_work;
extern uint32_t g_light_dst_n;

// ---- the command-list hooks the census owns -------------------------------------------------
// patch_command_list_vtable installs these but does not define them: a vtable SLOT has no
// registry, so the patcher has to name every hook, which is the coupling CVR_DETOUR removed for
// the RVA detours. Until a slot registry exists, they are declared here.
void STDMETHODCALLTYPE hk_Dispatch(ID3D12GraphicsCommandList*, UINT, UINT, UINT);
void STDMETHODCALLTYPE hk_ExecuteIndirect(ID3D12GraphicsCommandList*, ID3D12CommandSignature*, UINT, ID3D12Resource*, UINT64, ID3D12Resource*, UINT64);
void STDMETHODCALLTYPE hk_DrawInstanced(ID3D12GraphicsCommandList*, UINT, UINT, UINT, UINT);
void STDMETHODCALLTYPE hk_SetPipelineState(ID3D12GraphicsCommandList*, ID3D12PipelineState*);
void STDMETHODCALLTYPE hk_IASetVertexBuffers(ID3D12GraphicsCommandList*, UINT, UINT, const D3D12_VERTEX_BUFFER_VIEW*);
void STDMETHODCALLTYPE hk_DrawIndexedInstanced(ID3D12GraphicsCommandList*, UINT, UINT, UINT, INT, UINT);
void STDMETHODCALLTYPE hk_ClearDepthStencilView(ID3D12GraphicsCommandList*, D3D12_CPU_DESCRIPTOR_HANDLE,
                                               D3D12_CLEAR_FLAGS, FLOAT, UINT8, UINT, const D3D12_RECT*);
void STDMETHODCALLTYPE hk_CopyBufferRegion(ID3D12GraphicsCommandList*, ID3D12Resource*, UINT64, ID3D12Resource*, UINT64, UINT64);

void expo_probe_copy(ID3D12GraphicsCommandList*, ID3D12Resource*, bool);
void expo_mirror(ID3D12GraphicsCommandList*, ID3D12Resource*, bool);
void expo_probe_report();
bool tile_is_grid(const D3D12_RESOURCE_DESC&);
void cull_count_note(ID3D12GraphicsCommandList*, ID3D12Resource*, uint32_t, bool);
void cull_count_report();
void tile_probe_copy(ID3D12GraphicsCommandList*, ID3D12Resource*, const D3D12_RESOURCE_DESC&, D3D12_RESOURCE_STATES, bool);
void tile_probe_report();
void pso_ids_record(void*, const D3D12_SHADER_BYTECODE&, const D3D12_SHADER_BYTECODE&);
uint64_t fnv1a(const void*, size_t);
void sight_ps_dump(const void*, size_t, const char*);
void buf_note(ID3D12Resource*, uint64_t, uint64_t);

const uint8_t* filled_cpu_for_va(uint64_t, uint64_t);   // defined with the sight probe

bool hud_blur_signature(const D3D12_RESOURCE_DESC& d, uint64_t hudWidth);
bool hud_cb_block_plausible(const uint8_t* p, float w, float h);
extern "C" __declspec(dllexport) int      CyberpunkVR_HudToSecondEye;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_VisionSnap;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_HudNodeProbe;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudSnapSkips;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudSnaps;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVisionSnaps;
// How long the last outline snapshot stays usable, ms. 0 = no limit. The HUD's equivalent is
// CyberpunkVR_HudMaxAgeMs and this one exists because it was a hardcoded 250 ms.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VisionMaxAgeMs;
extern ID3D12Resource* g_hud_res;
extern bool g_hud_batch_listed;
extern bool g_rtv_dim_wrapped_logged;
extern std::array<RtvDimEntry, 8192> g_rtv_dim_map;
extern std::atomic<bool> g_hud_snap_fresh;
extern std::atomic<uint32_t> g_hud_last_mip;
extern std::atomic<uint32_t> g_rtv_dim_count;
extern std::atomic<uint64_t> g_hud_consumed_tick;
extern std::atomic<uint64_t> g_hud_node_binds;
extern std::atomic<uint64_t> g_hud_node_unresolved;
extern std::atomic<uint64_t> g_hud_snap_tick;
extern std::atomic<uint64_t> g_vision_tick;
extern uint64_t g_hud_cb_scan_tick;
extern uint8_t* g_hud_cb_copy_ptr;
void hud_cb_rescan();
void hud_node_note(ID3D12Resource* res, bool vrcam);
void hud_register_rtv(ID3D12Resource* res, const D3D12_RENDER_TARGET_VIEW_DESC* vd, D3D12_CPU_DESCRIPTOR_HANDLE h);
void hud_snapshot_copy(ID3D12GraphicsCommandList* list, ID3D12Resource* src, int which, D3D12_RESOURCE_STATES rest);

extern ID3D12Resource* g_hud_cb_copy;
extern std::array<MappedUpload, 64> g_upload_maps;
extern std::mutex g_upload_map_mtx;
extern uint32_t g_upload_map_n;

extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugExecTotal;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorGameQueue;
extern ID3D12CommandQueue* g_game_queue;
extern PFN_CreateCommandQueue  g_orig_CreateCommandQueue;
extern std::atomic<bool>     g_queue_vtable_patched;
extern std::atomic<uint64_t> g_exec_total;
extern std::mutex g_game_object_mtx;
void STDMETHODCALLTYPE hk_OMSetRenderTargets( ID3D12GraphicsCommandList* self, UINT count, const D3D12_CPU_DESCRIPTOR_HANDLE* handles, BOOL contiguous, const D3D12_CPU_DESCRIPTOR_HANDLE* depth);
void STDMETHODCALLTYPE hk_RSSetViewports( ID3D12GraphicsCommandList* self, UINT count, const D3D12_VIEWPORT* vps);
void STDMETHODCALLTYPE hk_ResourceBarrier(ID3D12GraphicsCommandList* self, UINT count, const D3D12_RESOURCE_BARRIER* barriers);


void hud_rearm_for_new_graph(uint64_t key);

void hud_adopt_by_node(ID3D12Resource*);

bool cbv_read_head(const uint8_t* p, float* xy, uint32_t* w);
bool vision_is_vrcam_full_size(const D3D12_RESOURCE_DESC& d);
bool vision_layer_signature(const D3D12_RESOURCE_DESC& d);
bool vision_matches_last_dispatch(const D3D12_RESOURCE_DESC& d);
extern "C" __declspec(dllexport) int32_t CyberpunkVR_CamCbProbe;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_CbvProbe;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_RtMapProbe;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_VisionMap;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_VisionPick;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CbvDumpNode;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VisionNode;
extern std::atomic<uint32_t> g_vrcam_view_h;
extern std::atomic<uint32_t> g_vrcam_view_w;
extern std::atomic<uint64_t> g_cc_big;
extern thread_local int32_t   t_vision_ord;
extern thread_local uintptr_t t_vision_node;
void camcb_note(const uint8_t* cp, bool vrcam);
void camcb_stages();
void cbv_dump_note(const uint8_t* p, bool vrcam);
void cbv_probe_note(uint32_t node_rva, uint32_t count, bool vrcam);
void rtmap_note(ID3D12Resource* res, bool vrcam);
void vision_note_surface(ID3D12Resource* res, bool vrcam, uint32_t node_rva, int32_t ord);

PFN_OMSetRenderTargets command_list_original_om( ID3D12GraphicsCommandList* command_list);
bool stereo_eye_capture_wanted();
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_HudByNode;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_LumaWave;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugLumaDeltaMilli;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugLumaEvenMilli;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugLumaOddMilli;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugStableSrcState;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugVrcamEyeAgeMs;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_HudNodeRva2;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_HudNodeRva;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_StereoEyeMaxAgeMs;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_Debug2RtBinds;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugExpoMain;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugExpoVrcam;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorRtvHits;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRttComp;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugStableCopies;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugStableSkips;
extern CreateCBVFn g_orig_CreateCBV;
extern D3D12_RESOURCE_DESC g_stable_desc;
extern std::atomic<SIZE_T> g_2rt_seen_h0[4];
extern std::atomic<SIZE_T> g_2rt_seen_h1[4];
extern std::atomic<SIZE_T> g_tonemap_h0;
extern std::atomic<uint32_t> g_rtv_dim_next;
extern std::atomic<uint64_t> g_eye_copy_calls;
extern std::atomic<uint64_t> g_eye_no_list;
extern std::atomic<uint64_t> g_eye_no_rtv;
extern std::atomic<uint64_t> g_eye_node_hits;
extern std::atomic<uint64_t> g_stable_tick;
extern std::atomic<uint64_t> g_stable_tick_us;
extern std::mutex g_gcb_mtx;
extern std::mutex g_rtv_dim_mtx;
extern thread_local DXGI_FORMAT t_mirror_copy_rtv_format;
extern thread_local ID3D12GraphicsCommandList* t_mirror_copy_list;
extern thread_local ID3D12GraphicsCommandList* t_tm_rt0_list;
extern thread_local ID3D12Resource* t_mirror_copy_rtv;
extern thread_local ID3D12Resource* t_tm_rt0;
extern thread_local bool t_mirror_copy_node_active;
extern thread_local bool t_tm_consumed;
extern thread_local uint32_t t_mirror_src_state;
// True only once hk_ResourceBarrier has seen a transition of that exact resource on this
// thread. A foreign resource is never given a barrier on a seeded or guessed state.
extern thread_local bool t_mirror_src_state_seen;
extern thread_local bool t_tm_rt0_state_seen;
extern thread_local uint32_t t_tm_rt0_state;
// The finished-frame path's per-thread slot -- see the note beside CyberpunkVR_FinalGrab in
// Capture.cpp for why these are thread_local and not globals.
extern thread_local bool t_tm_from_final;
extern thread_local ID3D12Resource*            t_final_res;
extern thread_local uint32_t                   t_final_state;
extern thread_local ID3D12GraphicsCommandList* t_final_list;
extern thread_local uint32_t                   t_final_writes;
void final_frame_copy_now();
extern uint32_t g_gcb_len[2][GRADE_CB_SLOTS];
extern uint8_t  g_gcb[2][GRADE_CB_SLOTS][GRADE_CB_MAX];
void tm_set_push(ID3D12Resource* res, uint32_t state, uint32_t fmt, uint32_t rva);

void STDMETHODCALLTYPE hk_CreateCBV(ID3D12Device*, const D3D12_CONSTANT_BUFFER_VIEW_DESC*, D3D12_CPU_DESCRIPTOR_HANDLE);

void STDMETHODCALLTYPE hk_ResourceBarrier(ID3D12GraphicsCommandList*, UINT, const D3D12_RESOURCE_BARRIER*);

void mirror_publish_output(ID3D12Resource*, DXGI_FORMAT);

void STDMETHODCALLTYPE hk_CreateRTV(ID3D12Device*, ID3D12Resource*, const D3D12_RENDER_TARGET_VIEW_DESC*, D3D12_CPU_DESCRIPTOR_HANDLE);

void STDMETHODCALLTYPE hk_OMSetRenderTargets(ID3D12GraphicsCommandList*, UINT, const D3D12_CPU_DESCRIPTOR_HANDLE*, BOOL, const D3D12_CPU_DESCRIPTOR_HANDLE*);

void mirror_stable_inline_copy(ID3D12GraphicsCommandList*, ID3D12Resource*, uint32_t src_state);

extern "C" __declspec(dllexport) uint32_t  CyberpunkVR_DebugDescHeapSVFlags;
extern "C" __declspec(dllexport) uint32_t  CyberpunkVR_DebugDescHeapSVNum;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugDescHeapResized;
extern "C" __declspec(dllexport) uint64_t  CyberpunkVR_DebugDescHeapCreates;
extern "C" __declspec(dllexport) uint64_t  CyberpunkVR_DebugDescHeapEnlarged;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugDescHeapSVRetAbs;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugDescHeapSVRetRva;
extern PFN_CreateDescriptorHeap  g_orig_CreateDescriptorHeap;
extern bool     g_enable_desc_heap_enlarge;
extern bool     g_enable_desc_heap_resize;
extern std::atomic<bool>         g_desc_vtable_patched;
extern std::atomic<bool>   g_desc_heap_resized;
extern uint32_t g_desc_heap_target;
void patch_descriptor_heap_size();
void patch_device_descriptor_slot(void* device);

bool node_cut_match(uint32_t rva, uint8_t rtid, bool vrcam);
extern "C" __declspec(dllexport) int      CyberpunkVR_HudGrantCap;
extern "C" __declspec(dllexport) int CyberpunkVR_HudBorrowBlocks;
extern "C" __declspec(dllexport) int CyberpunkVR_HudInVrcam;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_NodeCutEnable;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_NodeCutRetVal;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_HudCapWord;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudBlockLent;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudBlockNull;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudBlockOk;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudCapGrants;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudGateDenied;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudGateForced;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudNodeMain;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudNodeVrcam;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugNodeCutSkips;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_HudCapBits;
extern HudViewDataFn g_hud_viewdata_get;
extern ProfNode g_prof_nodes[512];
extern ProfPass g_prof_scenepass[256];
extern ViewFeatureCheckFn g_view_feature_check_orig;
extern bool g_hud_mask_dumped_main;
extern bool g_hud_mask_dumped_vrcam;
extern const uint32_t DRAWHUD_WORK_RVA;
extern const uint32_t SCENE_DRIVER_WORK_RVA;
extern const uintptr_t HUD_REQUIRED_MASK_RVA;
extern const uintptr_t VIEW_FEATURE_CHECK_RVA;
extern std::atomic<uint32_t> g_prof_ord_main;
extern std::atomic<uint32_t> g_prof_ord_vrcam;
extern void* g_hud_block_main;
void hud_dump_capability_mask(uint8_t* work_context, bool vrcam);
void hud_grant_capability(uintptr_t ctx);
void prof_node_add(uintptr_t work, int64_t dt, int64_t self, bool vrcam);
void prof_pair_add(uint8_t rtid, uint32_t rva, int64_t self, bool vrcam, bool nested, bool owner);

extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamFlagMode;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainUpscalerGroups;

extern "C" __declspec(dllexport) float    CyberpunkVR_DebugVrcamBaseFov;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugRttDtexH;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugRttDtexW;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugRttH;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugRttW;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_RttResizeH;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_RttResizeMatchMain;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_RttResizeW;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRttCompRejects;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRttHits;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRttResizeHits;
extern HandleAssignFn g_handle_assign;
extern ResizeDynTexFn g_resize_dyntex;
extern RttViewCreateFn g_orig_rtt_viewcreate;
extern bool g_rtt_res_override;
extern const RenderMaskEntry kRenderMasks[];
extern const uint32_t kRenderMaskCount;
extern std::atomic<uintptr_t> g_vrcam_ctx_seen;
extern uint32_t g_rtt_h;
extern uint32_t g_rtt_w;

void render_mask_report();     // defined further down, where g_main_ctx exists

extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBlockV5ReuseHits;
extern DoCullingFn g_orig_doculling;
extern DrawCompFn g_orig_drawcomp;
extern void* g_main_block_v5;

char __fastcall Detour_CloudsNode(void* a1, void* a2);
extern "C" __declspec(dllexport) int32_t CyberpunkVR_StableFromTonemap;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_FinalGrab;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMainNodeUnique;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugSecondaryNodeUnique;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainObjBinds;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorCopyArms;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorCopyNodeHits;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugTonemapSnaps;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugMainNodeWorks[256];
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugSecondaryNodeWorks[256];
extern CloudsNodeFn g_orig_clouds_node;
extern NodeDispatchFn          g_node_dispatch_orig;
extern WaitOnAddressFn         g_wait_on_address;
extern WakeByAddressAllFn      g_wake_by_address_all;
extern std::atomic<bool>       g_node_dispatch_hooked;
extern std::atomic<bool> g_have_tonemap_source;
extern thread_local bool     t_active_view_known;
extern thread_local int t_prof_disp_depth;
extern thread_local int64_t t_prof_child_ticks;
extern thread_local uint64_t t_active_view_key;

// WHICH VIEW IS RECORDING, ANSWERED THE ONLY WAY THAT HOLDS. The hash at ctx+0x28 reads 0 for
// MAIN and g_vrcam_ctx_key for the port's VRCAM; a reflection-probe face and the flat 960x540
// desktop window each have their own. `t_vrcam_node_active` answers only "is this VRCAM", so its
// else-branch lumps MAIN together with the desktop window and every probe face -- and any code
// that captures a MAIN REFERENCE on that branch is capturing whichever of them came last.
//
// That is not theoretical. Measured 2026-08-30 in the debugger, standing in a braindance with the
// green cast plainly on MAIN's screen, the port's captured "MAIN" grading block read the identity
// on one sample and the braindance grading on the next, one run/pause apart with nothing else
// changing. The exposure buffer had the same defect and the same day.
//
// Use these wherever a per-view reference is TAKEN or APPLIED. A view that is neither -- or one
// that could not be resolved -- must be left alone rather than counted as MAIN.
inline bool view_is_main_now() {
    return t_active_view_known && t_active_view_key == 0;
}
inline bool view_is_vrcam_now() {
    const uint64_t k = g_vrcam_ctx_key.load(std::memory_order_acquire);
    return t_active_view_known && k != 0 && t_active_view_key == k;
}
uint8_t __fastcall Detour_NodeDispatch( uintptr_t* node, uint8_t* work_context, void* args);
uint8_t __fastcall Detour_ViewFeatureCheck(uintptr_t work_context, uintptr_t required);

}  // namespace detail
}  // namespace cvr
