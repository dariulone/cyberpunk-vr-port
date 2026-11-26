#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Utils/DebugGate.hpp"
#include "Stereo/RenderParity.hpp"
#include "Render/NativePreparation.hpp"
#include "Render/NativeDepthPreparation.hpp"
#include "Render/NativeCpuCollector.hpp"
#include "Core/VrCoreShared.hpp"
// CullVisibility -- the engine's culling, visibility and prepare stages, per view.
//
// Sixteen detours and their own histogram/bucket helpers, all answering one question: for THIS view,
// what is visible and what has to be prepared for it. This is where the second eye either reuses the
// first view's answer or pays for its own, so it is also where most of the two-view CPU cost lives --
// which is what the profiler next door exists to measure.
//
// Lifted out of src/Stereo/SyncStereo.cpp as one band, because it IS one band: everything between
// Detour_GatherCtxInit and Detour_QueryWork belongs to this subsystem, including the six prepare_*
// helpers that nothing else calls. The detours register themselves through Stereo/DetourRegistry.hpp,
// so no install pass had to be edited to move them.

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
#include "Stereo/DetourRegistry.hpp"

namespace cvr {
namespace detail {

#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
void* __fastcall Detour_GatherCtxInit(void* ctx, uintptr_t view, void* cull_query, uint8_t flag) {
    void* const r = g_orig_gather_ctx_init(ctx, view, cull_query, flag);
    if (CyberpunkVR_LodThreshOverrideEnable && ctx && view) {
        __try {
            const uint64_t key = *reinterpret_cast<uint64_t*>(view + 0x28);
            const float aspect = *reinterpret_cast<float*>(view + 0x98);
            const uint32_t seen = *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(ctx) + 0x28);
            uint32_t bit;
            if (key == g_vrcam_ctx_key) { bit = 2; CyberpunkVR_DebugLodThreshSeenVrcamBits = seen; }
            else if (is_main_view(reinterpret_cast<void*>(view))) { bit = 1; CyberpunkVR_DebugLodThreshSeenMainBits = seen; }
            else bit = 4;
            if (CyberpunkVR_LodThreshApplyMask & bit) {
                *reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(ctx) + 0x28) = CyberpunkVR_LodThreshValue;
                if (bit == 2) CVR_DIAGNOSTIC(++CyberpunkVR_DebugLodThreshHitsVrcam);
                else if (bit == 1) CVR_DIAGNOSTIC(++CyberpunkVR_DebugLodThreshHitsMain);
                else CVR_DIAGNOSTIC(++CyberpunkVR_DebugLodThreshHitsOther);
            }
            lod_thresh_report();
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return r;
}
#endif

// ---- occlusion gate force ---------------------------------------------------------------
// sub_1405AC174 is visQuerySingleFrustum's vfn+0x20 (per-view visibility prepare). It gates
// the whole CPU software-occlusion pipeline on ONE byte:
//     if (*(BYTE*)(this + 0x19E)) { ... sub_14079F518(this + 0x240, ...); }
// and sub_14079F518 writes both this+0x240 (occ-ctx) and this+0x240+546 == this+0x462 (flag),
// which are exactly the two fields the coarse (sub_14029CEE4) and fine (sub_14029D110) tests
// check before doing an occlusion query. Measured live: MAIN has 0x19E==1, VRCAM has 0.
// Forcing it to 1 makes the engine build VRCAM its OWN rasterized depth buffer from VRCAM's
// OWN frustum -> correct occlusion, no cross-eye parallax error. The occluder list itself is
// scene-wide (this+8 -> +0x80, 604 entries observed for every caller) and sub_14079F518
// already early-outs when that list is empty, so this stays safe for views without occluders.
#if !CVR_RUST_VRIK
constexpr uintptr_t VIS_QUERY_PREPARE_RVA = 0x5AC174;
constexpr uintptr_t TESTER_OCC_GATE_OFF = 0x19E;
#endif
#if !CVR_RUST_VRIK
using VisQueryPrepareFn = __int64(__fastcall*)(void*, void*);
#endif
#if !CVR_RUST_VRIK
VisQueryPrepareFn g_orig_visquery_prepare = nullptr;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/stereo_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_OcclusionGateForce = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugOcclGateForced = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugOcclGateAlreadyOn = 0;
#endif

#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
__int64 __fastcall Detour_VisQueryPrepare(void* tester, void* job) {
    if (CyberpunkVR_OcclusionGateForce && tester) {
        __try {
            auto* const gate = reinterpret_cast<uint8_t*>(tester) + TESTER_OCC_GATE_OFF;
            if (*gate) {
                CVR_DIAGNOSTIC(++CyberpunkVR_DebugOcclGateAlreadyOn);
            } else {
                *gate = 1;
                CVR_DIAGNOSTIC(++CyberpunkVR_DebugOcclGateForced);
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return g_orig_visquery_prepare(tester, job);
}
#endif

#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
static bool visibility_replay_enabled() {
    return CyberpunkVR_CullReuseMode == 7 || CyberpunkVR_CullReuseMode == 8 ||
        CyberpunkVR_CullReuseMode == 9;
}

__int64 __fastcall Detour_MainCullPrepare(void* manager, void* job, void* output) {
    if (CyberpunkVR_CullCallbackProfileEnable && manager && job && output && g_exe_base) {
        if (!g_main_cull_ctx_init)
            g_main_cull_ctx_init = reinterpret_cast<MainCullCtxInitFn>(g_exe_base + MAIN_CULL_CTX_INIT_RVA);
        if (g_main_cull_ctx_init) {
            uint8_t* view = *reinterpret_cast<uint8_t**>(reinterpret_cast<uint8_t*>(job) + 0x18);
            uint32_t view_kind = 0;
            if (view) {
                const uint64_t key = *reinterpret_cast<uint64_t*>(view + 0x28);
                if (key == g_vrcam_ctx_key)
                    view_kind = 1;
                else if (is_main_view(view))
                    view_kind = 2;
            }
            alignas(16) uint8_t gather_ctx[72] = {};
            g_main_cull_ctx_init(gather_ctx,
                *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(job) + 24), output, 0);
            auto** callbacks = *reinterpret_cast<uintptr_t***>(reinterpret_cast<uint8_t*>(manager) + 516512);
            const uint32_t count = *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(manager) + 516524);
            __int64 result = 0;
            for (uint32_t i = 0; callbacks && i < count && i < CULL_CALLBACK_MAX; ++i) {
                uintptr_t obj = reinterpret_cast<uintptr_t>(callbacks[i]);
                if (!obj) continue;
                uintptr_t method = *reinterpret_cast<uintptr_t*>(*reinterpret_cast<uintptr_t*>(obj) + 248);
                const uintptr_t rva = method - reinterpret_cast<uintptr_t>(g_exe_base);
                CyberpunkVR_DebugCullCallbackMethodRva[i] = rva;
                const uint32_t before = *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(output) + 12);
                const int64_t t0 = prof_now();
                result = reinterpret_cast<__int64(__fastcall*)(uintptr_t, void*)>(method)(obj, gather_ctx);
                const uint64_t dt = static_cast<uint64_t>(prof_now() - t0);
                const uint32_t after = *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(output) + 12);
                const uint32_t delta = after >= before ? (after - before) : 0;
                if (view_kind == 1) {
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugCullCallbackCallsVrcam[i]);
                    CVR_DIAGNOSTIC(CyberpunkVR_DebugCullCallbackDescVrcam[i] += delta);
                    CVR_DIAGNOSTIC(CyberpunkVR_DebugCullCallbackTicksVrcam[i] += dt);
                } else if (view_kind == 2) {
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugCullCallbackCallsMain[i]);
                    CVR_DIAGNOSTIC(CyberpunkVR_DebugCullCallbackDescMain[i] += delta);
                    CVR_DIAGNOSTIC(CyberpunkVR_DebugCullCallbackTicksMain[i] += dt);
                }
            }
            return result;
        }
    }
    if ((CyberpunkVR_CullReuseMode == 8 || CyberpunkVR_CullReuseMode == 9) && job) {
        auto* const view = *reinterpret_cast<uint8_t**>(
            reinterpret_cast<uint8_t*>(job) + 0x18);
        if (is_main_view(view)) {
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugMainCullPrepareSkips);
            return 0;
        }
    }
    return g_orig_main_cull_prepare(manager, job, output);
}

char __fastcall Detour_VisibleAppend(void* output, uintptr_t* drawable_id) {
    if (CyberpunkVR_MaterializeProfileEnable)
        materialize_prof_add(CyberpunkVR_DebugMaterializeDrawableAppendsMain,
            CyberpunkVR_DebugMaterializeDrawableAppendsVrcam, 1);
    if (CyberpunkVR_CullReuseMode == 9 && t_capture_fine_ids && drawable_id)
        t_fine_ids.push_back(*drawable_id);
    return g_orig_visible_append(output, drawable_id);
}

char __fastcall Detour_FineMaterialize(
        void* tester, __int64* range, char partial, void* output) {
    char capturedResult{};
    if(cvr::stereo::CaptureCpuFine(tester,range,partial,output,capturedResult))return capturedResult;
    if (CyberpunkVR_MaterializeProfileEnable && range && range[0]) {
        const uint64_t item_count = static_cast<uint64_t>((range[1] - range[0]) / 40);
        const uint64_t range_hash = prepare_mix64(static_cast<uint64_t>(range[0])) ^
            prepare_mix64(static_cast<uint64_t>(range[1]) + 0x9E3779B97F4A7C15ull);
        const uint64_t range_ctx_hash = range_hash ^
            prepare_mix64(t_materialize_output_key + 0xD6E8FEB86659FD93ull);
        materialize_prof_add(CyberpunkVR_DebugMaterializeFineCallsMain,
            CyberpunkVR_DebugMaterializeFineCallsVrcam, 1);
        materialize_prof_add(CyberpunkVR_DebugMaterializeFineRangesMain,
            CyberpunkVR_DebugMaterializeFineRangesVrcam, item_count);
        materialize_range_observe(range_hash);
        materialize_range_ctx_observe(range_ctx_hash);
    }
    if (CyberpunkVR_CullReuseMode != 9 || !range || !range[0])
        return g_orig_fine_materialize(tester, range, partial, output);

    const uintptr_t candidate_key = static_cast<uintptr_t>(range[0]);
    const uint32_t phase = g_fine_reuse_phase.load(std::memory_order_acquire);
    if (phase == FINE_REUSE_CAPTURE) {
        const bool previous_capture = t_capture_fine_ids;
        t_capture_fine_ids = true;
        t_fine_ids.clear();
        if (t_fine_ids.capacity() < 64)
            t_fine_ids.reserve(64);
        const char result = g_orig_fine_materialize(tester, range, partial, output);
        t_capture_fine_ids = previous_capture;
        if (!previous_capture) {
            std::lock_guard<std::mutex> lock(g_fine_visibility_mutex);
            g_fine_visible_ids[candidate_key] = t_fine_ids;
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugFineCandidateCaptures);
            CVR_DIAGNOSTIC(CyberpunkVR_DebugFineDrawableIdsCaptured += t_fine_ids.size());
        }
        return result;
    }

    if (phase == FINE_REUSE_REPLAY && g_orig_visible_append) {
        std::vector<uintptr_t> ids;
        bool found = false;
        {
            std::lock_guard<std::mutex> lock(g_fine_visibility_mutex);
            const auto it = g_fine_visible_ids.find(candidate_key);
            if (it != g_fine_visible_ids.end()) {
                ids = it->second;
                found = true;
            }
        }
        if (found) {
            for (uintptr_t id : ids)
                g_orig_visible_append(output, &id);
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugFineCandidateReplays);
            CVR_DIAGNOSTIC(CyberpunkVR_DebugFineDrawableIdsReplayed += ids.size());
            return 0;
        }
        CVR_DIAGNOSTIC(++CyberpunkVR_DebugFineCandidateFallbacks);
    }
    return g_orig_fine_materialize(tester, range, partial, output);
}

__int64 __fastcall Detour_VisibilityCollector(void* context, void* batch_ptr) {
    if (visibility_replay_enabled() && t_capture_vrcam_visibility && batch_ptr) {
        auto* const batch = reinterpret_cast<ReplayVisibilityBatch*>(batch_ptr);
        const uint32_t count = (std::min)(batch->count, 32u);
        if (count) {
            ReplayVisibilityBatch cached{};
            cached.count = count;
            memcpy(cached.tags, batch->tags, sizeof(uintptr_t) * count);
            t_vrcam_visibility_batches.push_back(cached);
            CVR_DIAGNOSTIC(CyberpunkVR_DebugVisibilityCandidatesCaptured += count);
        }
    }
    return g_orig_visibility_collector(context, batch_ptr);
}

__int64 __fastcall Detour_MainCullTest(
        void* manager, void* job, void* output, void* tester, void* query) {
    int64_t capturedResult{};
    if(cvr::stereo::CaptureCpuScene(manager,job,output,tester,query,capturedResult))return capturedResult;
    if (!visibility_replay_enabled() || !job)
        return g_orig_main_cull_test(manager, job, output, tester, query);

    uint8_t* view = nullptr;
    uint64_t key = 0;
    float aspect = 0.0f;
    view = *reinterpret_cast<uint8_t**>(reinterpret_cast<uint8_t*>(job) + 0x18);
    if (!view)
        return g_orig_main_cull_test(manager, job, output, tester, query);
    key = *reinterpret_cast<uint64_t*>(view + 0x28);
    aspect = *reinterpret_cast<float*>(view + 0x98);

    if (key == g_vrcam_ctx_key) {
        if (CyberpunkVR_CullReuseMode == 9)
            g_fine_reuse_phase.store(FINE_REUSE_CAPTURE, std::memory_order_release);
        const bool previous_capture = t_capture_vrcam_visibility;
        t_capture_vrcam_visibility = true;
        t_vrcam_visibility_batches.clear();
        if (t_vrcam_visibility_batches.capacity() < 128)
            t_vrcam_visibility_batches.reserve(128);
        const __int64 result = g_orig_main_cull_test(manager, job, output, tester, query);
        t_capture_vrcam_visibility = previous_capture;
        if (!previous_capture) {
            {
                std::lock_guard<std::mutex> lock(g_visibility_batches_mutex);
                g_vrcam_visibility_batches = t_vrcam_visibility_batches;
            }
            g_vrcam_visibility_generation.fetch_add(1, std::memory_order_release);
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugVisibilityBatchCaptures);
        }
        return result;
    }

    if (is_main_view(view) &&
        g_vrcam_visibility_generation.load(std::memory_order_acquire) != 0 &&
        g_orig_visibility_collector) {
        if (CyberpunkVR_CullReuseMode == 9)
            g_fine_reuse_phase.store(FINE_REUSE_REPLAY, std::memory_order_release);
        std::vector<ReplayVisibilityBatch> batches;
        {
            std::lock_guard<std::mutex> lock(g_visibility_batches_mutex);
            batches = g_vrcam_visibility_batches;
        }
        if (!batches.empty()) {
            uintptr_t replay_context[5] = {
                reinterpret_cast<uintptr_t>(job),
                reinterpret_cast<uintptr_t>(output),
                reinterpret_cast<uintptr_t>(tester),
                reinterpret_cast<uintptr_t>(query),
                *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(manager) + 7224),
            };
            __int64 result = 0;
            uint64_t replayed = 0;
            for (auto& batch : batches) {
                replayed += batch.count;
                result = g_orig_visibility_collector(replay_context, &batch);
            }
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugVisibilityBatchReplays);
            CVR_DIAGNOSTIC(CyberpunkVR_DebugVisibilityCandidatesReplayed += replayed);
            return result;
        }
    }
    if (CyberpunkVR_CullReuseMode != 9)
        g_fine_reuse_phase.store(FINE_REUSE_IDLE, std::memory_order_release);
    return g_orig_main_cull_test(manager, job, output, tester, query);
}
#endif

// --- PrepareRenderElements stage profiler (diagnostic, default OFF) --------
#if !CVR_RUST_VRIK
using PrepareStageFn = void(__fastcall*)(void*, void*, void*, uint32_t, uint32_t);
using PrepareGatherFn = void*(__fastcall*)(void*, void*);
using PrepareFilterFn = __int64(__fastcall*)(void*, char, uint32_t, uint32_t);
using PrepareFinalizeFn = void(__fastcall*)(void*, char, __int64, __int64);
using PrepareSortFn = void(__fastcall*)(void*, void*, uint32_t, void*);
#endif
#if !CVR_RUST_VRIK
PrepareStageFn g_orig_prepare_stage = nullptr;
#endif
#if !CVR_RUST_VRIK
PrepareGatherFn g_orig_prepare_gather = nullptr;
#endif
#if !CVR_RUST_VRIK
PrepareFilterFn g_orig_prepare_filter = nullptr;
#endif
#if !CVR_RUST_VRIK
PrepareFinalizeFn g_orig_prepare_finalize = nullptr;
#endif
#if !CVR_RUST_VRIK
PrepareSortFn g_orig_prepare_sort_a = nullptr;
#endif
#if !CVR_RUST_VRIK
PrepareSortFn g_orig_prepare_sort_b = nullptr;
#endif
#if !CVR_RUST_VRIK
PrepareSortFn g_orig_prepare_sort_c = nullptr;
#endif
#if !CVR_RUST_VRIK
PrepareSortFn g_orig_prepare_sort_final = nullptr;
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
static thread_local uint32_t t_prepare_view_kind = 0; // 1=VRCAM, 2=MAIN gameplay
static thread_local uintptr_t t_prepare_bucket_key = 0;
static thread_local uint32_t t_prepare_stage_id = 0;
static thread_local uint8_t t_prepare_mode = 0;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/stereo_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_PrepareProfileEnable = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_PrepareCacheAuditEnable = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareStageTicksMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareStageTicksVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareGatherTicksMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareGatherTicksVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareFilterTicksMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareFilterTicksVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareFinalizeTicksMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareFinalizeTicksVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareSortATicksMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareSortATicksVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareSortBTicksMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareSortBTicksVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareSortCTicksMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareSortCTicksVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareSortFinalTicksMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareSortFinalTicksVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareCallsMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareCallsVrcam = 0;
#endif
#if !CVR_RUST_VRIK
constexpr uint32_t PREPARE_STAGE_MAX = 64;
constexpr uint32_t PREPARE_MODE_MAX = 4;
#endif
#if !CVR_RUST_VRIK
constexpr uint32_t PREPARE_STAGE_MODE_COUNT = PREPARE_STAGE_MAX * PREPARE_MODE_MAX;
constexpr uint32_t PREPARE_HIST_BUCKETS = 10;
constexpr uint32_t PREPARE_TOP_BUCKETS = 24;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/stereo_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugPrepareTopBucketCount = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugPrepareTopBucketStage[PREPARE_TOP_BUCKETS] = {};
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugPrepareTopBucketMode[PREPARE_TOP_BUCKETS] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareTopBucketTicksMain[PREPARE_TOP_BUCKETS] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareTopBucketTicksVrcam[PREPARE_TOP_BUCKETS] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareTopBucketDescMain[PREPARE_TOP_BUCKETS] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareTopBucketDescVrcam[PREPARE_TOP_BUCKETS] = {};
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugPrepareTopBucketCallsMain[PREPARE_TOP_BUCKETS] = {};
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugPrepareTopBucketCallsVrcam[PREPARE_TOP_BUCKETS] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareHistCallsMain[PREPARE_HIST_BUCKETS] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareHistCallsVrcam[PREPARE_HIST_BUCKETS] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareHistDescMain[PREPARE_HIST_BUCKETS] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareHistDescVrcam[PREPARE_HIST_BUCKETS] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareHistTicksMain[PREPARE_HIST_BUCKETS] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareHistTicksVrcam[PREPARE_HIST_BUCKETS] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketCallsMain[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketCallsVrcam[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketDescMain[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketDescVrcam[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketTicksMain[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketTicksVrcam[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketSortATicksMain[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketSortATicksVrcam[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketSortBTicksMain[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketSortBTicksVrcam[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketSortCTicksMain[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketSortCTicksVrcam[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketSortFinalTicksMain[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketSortFinalTicksVrcam[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketCountSqMain[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareBucketCountSqVrcam[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareFilterInMain[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareFilterInVrcam[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareFilterOutMain[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareFilterOutVrcam[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareFilterCallsMain[PREPARE_STAGE_MODE_COUNT] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareFilterCallsVrcam[PREPARE_STAGE_MODE_COUNT] = {};
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
struct PrepareFinalizeBucketStat {
    std::atomic<uint64_t> key{0};
    std::atomic<int64_t> ticks_main{0};
    std::atomic<int64_t> ticks_vrcam{0};
    std::atomic<uint64_t> desc_main{0};
    std::atomic<uint64_t> desc_vrcam{0};
    std::atomic<uint32_t> calls_main{0};
    std::atomic<uint32_t> calls_vrcam{0};
};
static PrepareFinalizeBucketStat g_prepare_finalize_buckets[128];
static uint32_t prepare_hist_bucket(uint32_t count) {
    if (count < 64) return 0;
    if (count < 128) return 1;
    if (count < 256) return 2;
    if (count < 512) return 3;
    if (count < 1024) return 4;
    if (count < 2048) return 5;
    if (count < 4096) return 6;
    if (count < 8192) return 7;
    if (count < 16384) return 8;
    return 9;
}

static uint32_t prepare_stage_mode_index(uint32_t stage, uint8_t mode) {
    if (stage >= PREPARE_STAGE_MAX || mode >= PREPARE_MODE_MAX)
        return 0xFFFFFFFFu;
    return stage * PREPARE_MODE_MAX + mode;
}

static void prepare_bucket_add(uint64_t* main_arr, uint64_t* vrcam_arr, uint32_t index, uint64_t value) {
    if (index == 0xFFFFFFFFu)
        return;
    if (t_prepare_view_kind == 1)
        InterlockedAdd64(reinterpret_cast<volatile LONG64*>(&vrcam_arr[index]), value);
    else if (t_prepare_view_kind == 2)
        InterlockedAdd64(reinterpret_cast<volatile LONG64*>(&main_arr[index]), value);
}

static PrepareFinalizeBucketStat* prepare_finalize_bucket(uint32_t stage_id, uint8_t mode) {
    const uint64_t want = (static_cast<uint64_t>(mode) << 32) | stage_id | 1ull;
    const size_t base = static_cast<size_t>((stage_id * 131u + mode * 17u) & 127u);
    for (size_t i = 0; i < std::size(g_prepare_finalize_buckets); ++i) {
        auto& slot = g_prepare_finalize_buckets[(base + i) & 127u];
        uint64_t cur = slot.key.load(std::memory_order_acquire);
        if (cur == want)
            return &slot;
        if (cur == 0 && slot.key.compare_exchange_strong(
                cur, want, std::memory_order_acq_rel, std::memory_order_acquire)) {
            return &slot;
        }
    }
    return nullptr;
}
#endif

#if CVR_RUST_VRIK
#else
extern "C" __declspec(dllexport) void CyberpunkVR_DumpPrepareFinalizeBuckets() {
    struct Row {
        uint32_t stage = 0;
        uint8_t mode = 0;
        int64_t tm = 0;
        int64_t tv = 0;
        uint64_t dm = 0;
        uint64_t dv = 0;
        uint32_t cm = 0;
        uint32_t cv = 0;
    } rows[128];
    int n = 0;
    for (auto& slot : g_prepare_finalize_buckets) {
        const uint64_t key = slot.key.load(std::memory_order_relaxed);
        if (!key)
            continue;
        auto& r = rows[n++];
        r.stage = static_cast<uint32_t>((key & 0xFFFFFFFFull) - 1ull);
        r.mode = static_cast<uint8_t>(key >> 32);
        r.tm = slot.ticks_main.exchange(0, std::memory_order_relaxed);
        r.tv = slot.ticks_vrcam.exchange(0, std::memory_order_relaxed);
        r.dm = slot.desc_main.exchange(0, std::memory_order_relaxed);
        r.dv = slot.desc_vrcam.exchange(0, std::memory_order_relaxed);
        r.cm = slot.calls_main.exchange(0, std::memory_order_relaxed);
        r.cv = slot.calls_vrcam.exchange(0, std::memory_order_relaxed);
        if (!(r.tm | r.tv | r.dm | r.dv | r.cm | r.cv))
            --n;
    }
    for (int i = 0; i < n; ++i) {
        int best = i;
        for (int j = i + 1; j < n; ++j)
            if (rows[j].tm + rows[j].tv > rows[best].tm + rows[best].tv)
                best = j;
        if (best != i) {
            Row tmp = rows[i]; rows[i] = rows[best]; rows[best] = tmp;
        }
    }
    memset(CyberpunkVR_DebugPrepareTopBucketStage, 0, sizeof(CyberpunkVR_DebugPrepareTopBucketStage));
    memset(CyberpunkVR_DebugPrepareTopBucketMode, 0, sizeof(CyberpunkVR_DebugPrepareTopBucketMode));
    memset(CyberpunkVR_DebugPrepareTopBucketTicksMain, 0, sizeof(CyberpunkVR_DebugPrepareTopBucketTicksMain));
    memset(CyberpunkVR_DebugPrepareTopBucketTicksVrcam, 0, sizeof(CyberpunkVR_DebugPrepareTopBucketTicksVrcam));
    memset(CyberpunkVR_DebugPrepareTopBucketDescMain, 0, sizeof(CyberpunkVR_DebugPrepareTopBucketDescMain));
    memset(CyberpunkVR_DebugPrepareTopBucketDescVrcam, 0, sizeof(CyberpunkVR_DebugPrepareTopBucketDescVrcam));
    memset(CyberpunkVR_DebugPrepareTopBucketCallsMain, 0, sizeof(CyberpunkVR_DebugPrepareTopBucketCallsMain));
    memset(CyberpunkVR_DebugPrepareTopBucketCallsVrcam, 0, sizeof(CyberpunkVR_DebugPrepareTopBucketCallsVrcam));
    CyberpunkVR_DebugPrepareTopBucketCount = 0;
    const int lim = n < static_cast<int>(PREPARE_TOP_BUCKETS)
        ? n : static_cast<int>(PREPARE_TOP_BUCKETS);
    for (int i = 0; i < lim; ++i) {
        CyberpunkVR_DebugPrepareTopBucketStage[i] = rows[i].stage;
        CyberpunkVR_DebugPrepareTopBucketMode[i] = rows[i].mode;
        CyberpunkVR_DebugPrepareTopBucketTicksMain[i] = static_cast<uint64_t>(rows[i].tm);
        CyberpunkVR_DebugPrepareTopBucketTicksVrcam[i] = static_cast<uint64_t>(rows[i].tv);
        CyberpunkVR_DebugPrepareTopBucketDescMain[i] = rows[i].dm;
        CyberpunkVR_DebugPrepareTopBucketDescVrcam[i] = rows[i].dv;
        CyberpunkVR_DebugPrepareTopBucketCallsMain[i] = rows[i].cm;
        CyberpunkVR_DebugPrepareTopBucketCallsVrcam[i] = rows[i].cv;
    }
    CyberpunkVR_DebugPrepareTopBucketCount = static_cast<uint32_t>(lim);
}
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/stereo_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareCacheHits = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareCacheMisses = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareCacheHitDescriptors = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareSetHits = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareSetMisses = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPrepareSetHitDescriptors = 0;
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
struct PrepareCacheAuditEntry {
    uint64_t hash = 0;
    uint64_t set_sum = 0;
    uint64_t set_xor = 0;
    uint32_t set_count = 0;
    std::vector<uint8_t> input;
};
static std::mutex g_prepare_cache_audit_mutex;
static std::unordered_map<uintptr_t, PrepareCacheAuditEntry> g_prepare_cache_audit;

static uint64_t prepare_descriptor_hash(const uint8_t* data, size_t size) {
    uint64_t hash = 1469598103934665603ull;
    for (size_t i = 0; i < size; ++i) {
        hash ^= data[i];
        hash *= 1099511628211ull;
    }
    return hash;
}

static uint64_t prepare_mix64(uint64_t value) {
    value ^= value >> 30;
    value *= 0xBF58476D1CE4E5B9ull;
    value ^= value >> 27;
    value *= 0x94D049BB133111EBull;
    return value ^ (value >> 31);
}

static void prepare_prof_add(uint64_t& main_ticks, uint64_t& vrcam_ticks, int64_t ticks) {
    if (t_prepare_view_kind == 1) {
        InterlockedAdd64(reinterpret_cast<volatile LONG64*>(&vrcam_ticks), ticks);
    } else if (t_prepare_view_kind == 2) {
        InterlockedAdd64(reinterpret_cast<volatile LONG64*>(&main_ticks), ticks);
    }
}
#endif

#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
void __fastcall Detour_PrepareStage(
        void* stage_context, void* buckets, void* output, uint32_t flags0, uint32_t flags1) {
    if(!CyberpunkVR_PrepareProfileEnable && !CyberpunkVR_PrepareCacheAuditEnable &&
        cvr::stereo::eager_depth::Intercept(g_orig_prepare_stage,g_orig_prepare_gather,g_orig_prepare_filter,g_orig_prepare_finalize,
            stage_context,buckets,output,flags0,flags1))return;
    const bool profile = CyberpunkVR_PrepareProfileEnable != 0;
    const bool cache_audit = CyberpunkVR_PrepareCacheAuditEnable != 0;
    if (!profile && !cache_audit && !cvr::stereo::preparation::Active()) {
        g_orig_prepare_stage(stage_context, buckets, output, flags0, flags1);
        return;
    }
    const uint32_t previous_kind = t_prepare_view_kind;
    const uintptr_t previous_bucket_key = t_prepare_bucket_key;
    const uint32_t previous_stage_id = t_prepare_stage_id;
    const uint8_t previous_mode = t_prepare_mode;
    t_prepare_view_kind = 0;
    t_prepare_stage_id = 0;
    t_prepare_mode = 0;
    t_prepare_bucket_key = static_cast<uintptr_t>(prepare_mix64(
        reinterpret_cast<uintptr_t>(buckets)) ^
        prepare_mix64(static_cast<uint64_t>(flags0) |
            (static_cast<uint64_t>(flags1) << 32)));
    __try {
        if (stage_context) {
            t_prepare_stage_id = *reinterpret_cast<uint32_t*>(
                reinterpret_cast<uint8_t*>(stage_context) + 0x14);
            t_prepare_mode = *reinterpret_cast<uint8_t*>(
                reinterpret_cast<uint8_t*>(stage_context) + 0x18);
            const uint64_t stage_signature =
                static_cast<uint64_t>(t_prepare_stage_id) |
                (static_cast<uint64_t>(t_prepare_mode) << 32) |
                (static_cast<uint64_t>(*reinterpret_cast<uint8_t*>(
                    reinterpret_cast<uint8_t*>(stage_context) + 0x19)) << 40);
            t_prepare_bucket_key ^= static_cast<uintptr_t>(prepare_mix64(stage_signature));
        }
        auto* render_context = *reinterpret_cast<uint8_t**>(stage_context);
        auto* view = render_context
            ? *reinterpret_cast<uint8_t**>(render_context + 0x18) : nullptr;
        if (view) {
            const uint64_t key = *reinterpret_cast<uint64_t*>(view + 0x28);
            if (key == g_vrcam_ctx_key) {
                t_prepare_view_kind = 1;
            } else if (is_main_view(view)) {
                t_prepare_view_kind = 2;
            }
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    const auto preparation=cvr::stereo::preparation::BeginStage(stage_context,buckets,output,flags0,flags1,
        t_prepare_view_kind==1?1:t_prepare_view_kind==2?0:-1);
    const int64_t t0 = profile ? prof_now() : 0;
    g_orig_prepare_stage(stage_context, buckets, output, flags0, flags1);
    if (profile) {
        const int64_t dt = prof_now() - t0;
        prepare_prof_add(CyberpunkVR_DebugPrepareStageTicksMain,
            CyberpunkVR_DebugPrepareStageTicksVrcam, dt);
        if (t_prepare_view_kind == 1)
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugPrepareCallsVrcam);
        else if (t_prepare_view_kind == 2)
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugPrepareCallsMain);
    }
    cvr::stereo::preparation::EndStage(preparation);
    t_prepare_view_kind = previous_kind;
    t_prepare_bucket_key = previous_bucket_key;
    t_prepare_stage_id = previous_stage_id;
    t_prepare_mode = previous_mode;
}

void* __fastcall Detour_PrepareGather(void* buckets, void* output) {
    if (!CyberpunkVR_PrepareProfileEnable || !t_prepare_view_kind) {
        void* result=g_orig_prepare_gather(buckets,output);cvr::stereo::preparation::Output(cvr::stereo::preparation::Kind::Gather,output);return result;
    }
    const int64_t t0 = prof_now();
    void* const result = g_orig_prepare_gather(buckets, output);
    prepare_prof_add(CyberpunkVR_DebugPrepareGatherTicksMain,
        CyberpunkVR_DebugPrepareGatherTicksVrcam, prof_now() - t0);
    cvr::stereo::preparation::Output(cvr::stereo::preparation::Kind::Gather,output);
    return result;
}

__int64 __fastcall Detour_PrepareFilter(
        void* output, char mode, uint32_t flags0, uint32_t flags1) {
    if (!CyberpunkVR_PrepareProfileEnable || !t_prepare_view_kind) {
        const auto result=g_orig_prepare_filter(output,mode,flags0,flags1);cvr::stereo::preparation::Output(cvr::stereo::preparation::Kind::Filter,output);return result;
    }
    const uint32_t count_before = output
        ? *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(output) + 0x0C)
        : 0;
    const uint32_t bucket_index = prepare_stage_mode_index(
        t_prepare_stage_id, static_cast<uint8_t>(mode));
    prepare_bucket_add(CyberpunkVR_DebugPrepareFilterInMain,
        CyberpunkVR_DebugPrepareFilterInVrcam, bucket_index, count_before);
    prepare_bucket_add(CyberpunkVR_DebugPrepareFilterCallsMain,
        CyberpunkVR_DebugPrepareFilterCallsVrcam, bucket_index, 1);
    const int64_t t0 = prof_now();
    const __int64 result = g_orig_prepare_filter(output, mode, flags0, flags1);
    const uint32_t count_after = output
        ? *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(output) + 0x0C)
        : 0;
    prepare_bucket_add(CyberpunkVR_DebugPrepareFilterOutMain,
        CyberpunkVR_DebugPrepareFilterOutVrcam, bucket_index, count_after);
    prepare_prof_add(CyberpunkVR_DebugPrepareFilterTicksMain,
        CyberpunkVR_DebugPrepareFilterTicksVrcam, prof_now() - t0);
    cvr::stereo::preparation::Output(cvr::stereo::preparation::Kind::Filter,output);
    return result;
}

void __fastcall Detour_PrepareFinalize(
        void* output, char mode, __int64 a3, __int64 a4) {
    if (CyberpunkVR_PrepareCacheAuditEnable && t_prepare_view_kind == 1 && mode == 1 &&
        t_prepare_bucket_key && output) {
        const uint32_t count = *reinterpret_cast<uint32_t*>(
            reinterpret_cast<uint8_t*>(output) + 0x0C);
        auto* const data = *reinterpret_cast<uint8_t**>(output);
        if (count && data) {
            const size_t bytes = static_cast<size_t>(count) * 16;
            const uint64_t hash = prepare_descriptor_hash(data, bytes);
            uint64_t set_sum = 0;
            uint64_t set_xor = 0;
            for (uint32_t i = 0; i < count; ++i) {
                const uint64_t a = *reinterpret_cast<const uint64_t*>(data + 16ull * i);
                const uint64_t b = *reinterpret_cast<const uint64_t*>(data + 16ull * i + 8);
                const uint64_t item_hash = prepare_mix64(a) ^ prepare_mix64(b + 0x9E3779B97F4A7C15ull);
                set_sum += item_hash;
                set_xor ^= prepare_mix64(item_hash + 0xD6E8FEB86659FD93ull);
            }
            bool hit = false;
            bool set_hit = false;
            {
                std::lock_guard<std::mutex> lock(g_prepare_cache_audit_mutex);
                auto& entry = g_prepare_cache_audit[t_prepare_bucket_key];
                hit = entry.hash == hash && entry.input.size() == bytes &&
                    memcmp(entry.input.data(), data, bytes) == 0;
                set_hit = entry.set_count == count && entry.set_sum == set_sum &&
                    entry.set_xor == set_xor;
                entry.hash = hash;
                entry.set_sum = set_sum;
                entry.set_xor = set_xor;
                entry.set_count = count;
                entry.input.assign(data, data + bytes);
            }
            if (hit) {
                CVR_DIAGNOSTIC(++CyberpunkVR_DebugPrepareCacheHits);
                CVR_DIAGNOSTIC(CyberpunkVR_DebugPrepareCacheHitDescriptors += count);
            } else {
                CVR_DIAGNOSTIC(++CyberpunkVR_DebugPrepareCacheMisses);
            }
            if (set_hit) {
                CVR_DIAGNOSTIC(++CyberpunkVR_DebugPrepareSetHits);
                CVR_DIAGNOSTIC(CyberpunkVR_DebugPrepareSetHitDescriptors += count);
            } else {
                CVR_DIAGNOSTIC(++CyberpunkVR_DebugPrepareSetMisses);
            }
        }
    }
    if (CyberpunkVR_PrepareProfileEnable && t_prepare_view_kind && output) {
        const uint32_t count = *reinterpret_cast<uint32_t*>(
            reinterpret_cast<uint8_t*>(output) + 0x0C);
        const uint32_t bucket_index = prepare_stage_mode_index(
            t_prepare_stage_id, static_cast<uint8_t>(mode));
        const uint32_t hist = prepare_hist_bucket(count);
        prepare_bucket_add(CyberpunkVR_DebugPrepareHistCallsMain,
            CyberpunkVR_DebugPrepareHistCallsVrcam, hist, 1);
        prepare_bucket_add(CyberpunkVR_DebugPrepareHistDescMain,
            CyberpunkVR_DebugPrepareHistDescVrcam, hist, count);
        prepare_bucket_add(CyberpunkVR_DebugPrepareBucketCallsMain,
            CyberpunkVR_DebugPrepareBucketCallsVrcam, bucket_index, 1);
        prepare_bucket_add(CyberpunkVR_DebugPrepareBucketDescMain,
            CyberpunkVR_DebugPrepareBucketDescVrcam, bucket_index, count);
        prepare_bucket_add(CyberpunkVR_DebugPrepareBucketCountSqMain,
            CyberpunkVR_DebugPrepareBucketCountSqVrcam, bucket_index,
            static_cast<uint64_t>(count) * static_cast<uint64_t>(count));
        if (auto* slot = prepare_finalize_bucket(t_prepare_stage_id, static_cast<uint8_t>(mode))) {
            if (t_prepare_view_kind == 1) {
                InterlockedAdd64(reinterpret_cast<volatile LONG64*>(&slot->desc_vrcam), count);
                slot->calls_vrcam.fetch_add(1, std::memory_order_relaxed);
            } else if (t_prepare_view_kind == 2) {
                InterlockedAdd64(reinterpret_cast<volatile LONG64*>(&slot->desc_main), count);
                slot->calls_main.fetch_add(1, std::memory_order_relaxed);
            }
        }
    }
    if (!CyberpunkVR_PrepareProfileEnable || !t_prepare_view_kind) {
        g_orig_prepare_finalize(output, mode, a3, a4);
        cvr::stereo::preparation::Output(cvr::stereo::preparation::Kind::Finalize,output);
        return;
    }
    const int64_t t0 = prof_now();
    g_orig_prepare_finalize(output, mode, a3, a4);
    const int64_t dt = prof_now() - t0;
    cvr::stereo::preparation::Output(cvr::stereo::preparation::Kind::Finalize,output);
    prepare_prof_add(CyberpunkVR_DebugPrepareFinalizeTicksMain,
        CyberpunkVR_DebugPrepareFinalizeTicksVrcam, dt);
    const uint32_t count = *reinterpret_cast<uint32_t*>(
        reinterpret_cast<uint8_t*>(output) + 0x0C);
    const uint32_t hist = prepare_hist_bucket(count);
    const uint32_t bucket_index = prepare_stage_mode_index(
        t_prepare_stage_id, static_cast<uint8_t>(mode));
    prepare_bucket_add(CyberpunkVR_DebugPrepareHistTicksMain,
        CyberpunkVR_DebugPrepareHistTicksVrcam, hist, static_cast<uint64_t>(dt));
    prepare_bucket_add(CyberpunkVR_DebugPrepareBucketTicksMain,
        CyberpunkVR_DebugPrepareBucketTicksVrcam, bucket_index, static_cast<uint64_t>(dt));
    if (auto* slot = prepare_finalize_bucket(t_prepare_stage_id, static_cast<uint8_t>(mode))) {
        if (t_prepare_view_kind == 1)
            InterlockedAdd64(reinterpret_cast<volatile LONG64*>(&slot->ticks_vrcam), dt);
        else if (t_prepare_view_kind == 2)
            InterlockedAdd64(reinterpret_cast<volatile LONG64*>(&slot->ticks_main), dt);
    }
}

void __fastcall Detour_PrepareSortA(
        void* begin, void* end, uint32_t count, void* keys) {
    if (!CyberpunkVR_PrepareProfileEnable || !t_prepare_view_kind) {
        g_orig_prepare_sort_a(begin, end, count, keys);
        return;
    }
    const int64_t t0 = prof_now();
    g_orig_prepare_sort_a(begin, end, count, keys);
    const uint64_t dt = static_cast<uint64_t>(prof_now() - t0);
    prepare_prof_add(CyberpunkVR_DebugPrepareSortATicksMain,
        CyberpunkVR_DebugPrepareSortATicksVrcam, dt);
    prepare_bucket_add(CyberpunkVR_DebugPrepareBucketSortATicksMain,
        CyberpunkVR_DebugPrepareBucketSortATicksVrcam,
        prepare_stage_mode_index(t_prepare_stage_id, t_prepare_mode),
        dt);
}

void __fastcall Detour_PrepareSortB(
        void* begin, void* end, uint32_t count, void* keys) {
    if (!CyberpunkVR_PrepareProfileEnable || !t_prepare_view_kind) {
        g_orig_prepare_sort_b(begin, end, count, keys);
        return;
    }
    const int64_t t0 = prof_now();
    g_orig_prepare_sort_b(begin, end, count, keys);
    const uint64_t dt = static_cast<uint64_t>(prof_now() - t0);
    prepare_prof_add(CyberpunkVR_DebugPrepareSortBTicksMain,
        CyberpunkVR_DebugPrepareSortBTicksVrcam, dt);
    prepare_bucket_add(CyberpunkVR_DebugPrepareBucketSortBTicksMain,
        CyberpunkVR_DebugPrepareBucketSortBTicksVrcam,
        prepare_stage_mode_index(t_prepare_stage_id, t_prepare_mode),
        dt);
}

void __fastcall Detour_PrepareSortC(
        void* begin, void* end, uint32_t count, void* keys) {
    if (!CyberpunkVR_PrepareProfileEnable || !t_prepare_view_kind) {
        g_orig_prepare_sort_c(begin, end, count, keys);
        return;
    }
    const int64_t t0 = prof_now();
    g_orig_prepare_sort_c(begin, end, count, keys);
    const uint64_t dt = static_cast<uint64_t>(prof_now() - t0);
    prepare_prof_add(CyberpunkVR_DebugPrepareSortCTicksMain,
        CyberpunkVR_DebugPrepareSortCTicksVrcam, dt);
    prepare_bucket_add(CyberpunkVR_DebugPrepareBucketSortCTicksMain,
        CyberpunkVR_DebugPrepareBucketSortCTicksVrcam,
        prepare_stage_mode_index(t_prepare_stage_id, t_prepare_mode),
        dt);
}

void __fastcall Detour_PrepareSortFinal(
        void* begin, void* end, uint32_t count, void* keys) {
    if (!CyberpunkVR_PrepareProfileEnable || !t_prepare_view_kind) {
        g_orig_prepare_sort_final(begin, end, count, keys);
        return;
    }
    const int64_t t0 = prof_now();
    g_orig_prepare_sort_final(begin, end, count, keys);
    const uint64_t dt = static_cast<uint64_t>(prof_now() - t0);
    prepare_prof_add(CyberpunkVR_DebugPrepareSortFinalTicksMain,
        CyberpunkVR_DebugPrepareSortFinalTicksVrcam, dt);
    prepare_bucket_add(CyberpunkVR_DebugPrepareBucketSortFinalTicksMain,
        CyberpunkVR_DebugPrepareBucketSortFinalTicksVrcam,
        prepare_stage_mode_index(t_prepare_stage_id, t_prepare_mode),
        dt);
}
#endif

// --- VRCAM localCtx test in multifrustum worker ---------------------------
// Live RE proved query+0x350 (global occlusion ctx) is ALREADY shared between MAIN and
// VRCAM. The only stable difference at worker sub_14029C158 is query+0x348:
//   MAIN  -> [query+0x348] == 0
//   VRCAM -> [query+0x348] != 0
// One-shot live nulling of VRCAM's +0x348 kept the scene intact, but FPS did not move in a
// single-frame poke. Mode 4 repeats that null EVERY invocation for a real measurement.
#if !CVR_RUST_VRIK
constexpr uintptr_t QUERYWORK_RVA = 0x29C158;   // sub_14029C158 multifrustum worker
#endif
#if !CVR_RUST_VRIK
using QueryWorkFn = __int64(__fastcall*)(void*, void*);
#endif
#if !CVR_RUST_VRIK
QueryWorkFn g_orig_querywork = nullptr;
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
__int64 __fastcall Detour_QueryWork(void* query, void* a2) {
    if (CyberpunkVR_CullReuseMode == 4 && query) {
        void** local_ctx = reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(query) + 0x348);
        void* saved = *local_ctx;
        if (saved) {
            *local_ctx = nullptr;
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugLocalCtxZeroHits);
            __int64 r = g_orig_querywork(query, a2);
            *local_ctx = saved;
            return r;
        }
    }
    return g_orig_querywork(query, a2);
}
#endif

// ---- these detours declare themselves, in the file that defines them -------------------------
// Culling and prepare detours that serve VRCAM (its cull reuse and replay), the profilers and the single-pass
// capture are not installed in engine stereo (Stereo/EngineStereo.hpp SecondViewWanted). The LOD threshold,
// the occlusion gate and the graph context stay: they act on MAIN and the scope camera's view as well.
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[cull] FineMaterialize sub_14029D110", FINE_MATERIALIZE_RVA, Detour_FineMaterialize, g_orig_fine_materialize, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR("[cull] GatherCtxInit sub_140623FD8 (LOD-thresh sweep)", MAIN_CULL_CTX_INIT_RVA, Detour_GatherCtxInit, g_orig_gather_ctx_init)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[cull] MainCullPrepare sub_140C65218", MAIN_CULL_PREP_RVA, Detour_MainCullPrepare, g_orig_main_cull_prepare, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[cull] MainCullTest sub_140C651C4", MAIN_CULL_TEST_RVA, Detour_MainCullTest, g_orig_main_cull_test, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[prep] filter sub_14229D240", PREPARE_FILTER_RVA, Detour_PrepareFilter, g_orig_prepare_filter, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[prep] finalize sub_140426AE4", PREPARE_FINALIZE_RVA, Detour_PrepareFinalize, g_orig_prepare_finalize, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[prep] gather sub_1404EE6BC", PREPARE_GATHER_RVA, Detour_PrepareGather, g_orig_prepare_gather, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[prep] sortA sub_140427AC8", PREPARE_SORT_A_RVA, Detour_PrepareSortA, g_orig_prepare_sort_a, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[prep] sortB sub_140427F00", PREPARE_SORT_B_RVA, Detour_PrepareSortB, g_orig_prepare_sort_b, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[prep] sortC sub_140428330", PREPARE_SORT_C_RVA, Detour_PrepareSortC, g_orig_prepare_sort_c, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[prep] finalSort sub_140695F80", PREPARE_SORT_FINAL_RVA, Detour_PrepareSortFinal, g_orig_prepare_sort_final, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[prep] stage sub_14229D3E0", PREPARE_STAGE_RVA, Detour_PrepareStage, g_orig_prepare_stage, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[cull] QueryWork sub_14029C158", QUERYWORK_RVA, Detour_QueryWork, g_orig_querywork, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[cull] VisibleAppend sub_14019B75C", VISIBLE_APPEND_RVA, Detour_VisibleAppend, g_orig_visible_append, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[cull] VisibilityCollector sub_1404ED29C", VIS_COLLECTOR_RVA, Detour_VisibilityCollector, g_orig_visibility_collector, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR("[cull] VisQueryPrepare sub_1405AC174 (occlusion gate)", VIS_QUERY_PREPARE_RVA, Detour_VisQueryPrepare, g_orig_visquery_prepare)
#endif

// ================================================================================================
// CULL REUSE, THE GATHER CONTEXT, AND THE LOD SWEEP, moved out of the monolith to sit with the culling
// they belong to.
//
// The reuse mode decides whether the second view gets its own cull or reads the first's. The gather
// context detours are how that choice is applied at the point the engine allocates and resets one. The
// LOD sweep overrides the detail threshold at gather-context+0x28, which is a measurement tool: it
// answers "does LOD explain the second view's cost" with a number.
//
// lod_thresh_report is kept because the override is silent when it does not take -- and an override
// that quietly does nothing looks exactly like a feature that does not help.
// ================================================================================================

#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CullReuseMode = 0;   // 0=off, 1=skip VRCAM cull, 2=skip MAIN cull (diag), 4=force VRCAM query localCtx(+0x348)=0, 5=REUSE MAIN block-list v5, 6=unsafe graph-output experiment, 7=replay VRCAM tagged visibility, 8=mode7 + skip duplicate MAIN candidate gather, 9=mode8 + replay fine drawable IDs
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/stereo_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullSkipHits = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLocalCtxZeroHits = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBlockReuseHits = 0;
// mode6: GraphContextPrepare reuses pool addresses sequentially. At entry its active list
// still names the PREVIOUS subgraph. Live order proved old=VRCAM -> GraphContextPrepare ->
// DoCulling MAIN. During that transition, preserve the entire VRCAM graph container by
// skipping sub_1404ECEA0 (which otherwise clears both visibility buckets and payload
// metadata), then skip MAIN's duplicate cull. MAIN draw consumes VRCAM's current-frame
// output. The next MAIN->VRCAM transition performs the normal reset, so nothing accumulates.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugContainerRedirectHits = 0;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugMainContainer = 0;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_DebugVrcamContainer = 0;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/stereo_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVisibilityResetSkipHits = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugEndRenderResetSkipHits = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainCullReuseHits = 0;
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
static std::atomic<bool> g_main_visibility_reuse_armed{false};
enum : uint32_t {
    VIS_REUSE_IDLE = 0,
    VIS_REUSE_VRCAM_ACTIVE = 1,
    VIS_REUSE_VRCAM_PRESERVED = 2,
    VIS_REUSE_MAIN = 3,
};
static std::atomic<uint32_t> g_visibility_reuse_phase{VIS_REUSE_IDLE};
static thread_local bool t_preserve_vrcam_graph = false;
static thread_local void* t_preserve_container = nullptr;
#endif

#if !CVR_RUST_VRIK
using GraphContextPrepareFn = void*(__fastcall*)(void*, void*, void*);
using GraphContextResetFn = __int64(__fastcall*)(void*);
#endif
#if CVR_RUST_VRIK
// The trampolines of DoCulling, GraphContextPrepare, MainCullTest and FineMaterialize are Rust's (cull_visibility.rs's
// cvr_cull_stage_originals, registered from engine_detours.rs); PreparePair and the CPU collector probe call through them.
#else
static GraphContextPrepareFn g_orig_graph_context_prepare = nullptr;
#endif
#if !CVR_RUST_VRIK
static GraphContextResetFn g_orig_graph_context_reset = nullptr;
#endif

#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
static void* __fastcall Detour_GraphContextPrepare(void* a1, void* a2, void* a3) {
    cvr::stereo::preparation::Graph(a1,false);
    cvr::stereo::ClearPreparedGraph();
    const bool previous_preserve = t_preserve_vrcam_graph;
    void* const previous_container = t_preserve_container;
    t_preserve_vrcam_graph = false;
    t_preserve_container = nullptr;

    if (CyberpunkVR_CullReuseMode == 6 && a1) {
        __try {
            const uint32_t count = *reinterpret_cast<uint32_t*>(
                reinterpret_cast<uint8_t*>(a1) + 0x54);
            auto** views = *reinterpret_cast<uint8_t***>(
                reinterpret_cast<uint8_t*>(a1) + 0x48);
            if (count == 1 && views && views[0]) {
                uint8_t* const old_view = views[0];
                const uint64_t key = *reinterpret_cast<uint64_t*>(old_view + 0x28);
                void* const container = *reinterpret_cast<void**>(old_view + 0x2C40);
                if (key == g_vrcam_ctx_key && container) {
                    // The prepare now rebuilding this manager is MAIN. Its reset must not
                    // destroy the VRCAM cull/draw payload we want MAIN to consume.
                    t_preserve_vrcam_graph = true;
                    t_preserve_container = container;
                    g_main_visibility_reuse_armed.store(false, std::memory_order_release);
                    CyberpunkVR_DebugVrcamContainer = reinterpret_cast<uintptr_t>(container);
                } else if (is_main_view(old_view)) {
                    // MAIN->VRCAM: allow the normal reset before VRCAM writes a fresh frame.
                    g_main_visibility_reuse_armed.store(false, std::memory_order_release);
                    g_visibility_reuse_phase.store(VIS_REUSE_IDLE, std::memory_order_release);
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    } else {
        g_main_visibility_reuse_armed.store(false, std::memory_order_release);
        g_visibility_reuse_phase.store(VIS_REUSE_IDLE, std::memory_order_release);
    }

    void* const result = cvr::stereo::preparation::PreparePair(g_orig_graph_context_prepare,a1,a2,a3,
        g_vrcam_ctx_key.load(std::memory_order_acquire),g_main_view_ctx.load(std::memory_order_acquire),
        g_menuModeValue==0 && CyberpunkVR_CullReuseMode==0 && cvr::stereo::preparation::PairPrepareCompatible(),
        g_vrcam_ctx_seen.load(std::memory_order_acquire));
    cvr::stereo::preparation::Graph(a1,true);
    cvr::stereo::ObservePreparedGraph(reinterpret_cast<uintptr_t>(a1));
    t_preserve_vrcam_graph = previous_preserve;
    t_preserve_container = previous_container;
    return result;
}

static __int64 __fastcall Detour_GraphContextReset(void* container) {
    const uintptr_t return_rva = g_exe_base
        ? reinterpret_cast<uintptr_t>(_ReturnAddress()) - reinterpret_cast<uintptr_t>(g_exe_base)
        : 0;
    const uint32_t phase = g_visibility_reuse_phase.load(std::memory_order_acquire);
    // CRenderNode_EndRender -> sub_14079A804 calls reset at 0x14079A853
    // (return RVA 0x8EB20A). Phase identifies whether this is VRCAM or MAIN.
    const bool preserve_end_render = CyberpunkVR_CullReuseMode == 6 &&
        return_rva == 0x8EB20A && phase == VIS_REUSE_VRCAM_ACTIVE;
    const bool preserve_main_prepare =
        CyberpunkVR_CullReuseMode == 6 && t_preserve_vrcam_graph &&
        container == t_preserve_container;
    if (preserve_end_render || preserve_main_prepare) {
        CVR_DIAGNOSTIC(++CyberpunkVR_DebugVisibilityResetSkipHits);
        if (preserve_end_render)
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugEndRenderResetSkipHits);
        CVR_DIAGNOSTIC(++CyberpunkVR_DebugContainerRedirectHits); // legacy counter kept for live telemetry
        if (preserve_end_render) {
            g_visibility_reuse_phase.store(
                VIS_REUSE_VRCAM_PRESERVED, std::memory_order_release);
        } else {
            g_visibility_reuse_phase.store(VIS_REUSE_MAIN, std::memory_order_release);
            g_main_visibility_reuse_armed.store(true, std::memory_order_release);
        }
        return 0;
    }
    if (CyberpunkVR_CullReuseMode == 6 && return_rva == 0x8EB20A &&
        phase == VIS_REUSE_MAIN) {
        // MAIN consumed the preserved data; its EndRender performs normal cleanup.
        g_main_visibility_reuse_armed.store(false, std::memory_order_release);
        g_visibility_reuse_phase.store(VIS_REUSE_IDLE, std::memory_order_release);
    }
    return g_orig_graph_context_reset(container);
}

// KEY TEST: visibility counts (node+0x20) are SET by cull, READ (not reset) by the
// extraction predicate sub_1402397B4. Frame order is VRCAM-first. If the counts PERSIST
// between the two views' extractions, then the SECOND view (MAIN) can skip its own cull
// and reuse the FIRST view's (VRCAM) counts -> MAIN shows the scene (mode 2). If instead
// MAIN domes, the counts are consumed/overwritten and reuse is blocked.
static bool doculling_is_vrcam(void* a2) {
    if (CyberpunkVR_CullReuseMode == 0 || !a2) return false;
    __try {
        uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
            reinterpret_cast<uint8_t*>(a2) + 0x18);
        if (!ctx) return false;
        const uint64_t key = *reinterpret_cast<uint64_t*>(ctx + 0x28);
        if (CyberpunkVR_CullReuseMode == 1 || CyberpunkVR_CullReuseMode == 5)
            return key == g_vrcam_ctx_key;                       // skip VRCAM's cull
        if (CyberpunkVR_CullReuseMode == 2)                    // skip MAIN gameplay cull
            return is_main_view(reinterpret_cast<void*>(ctx)); // gameplay main only (not helpers)
        return false;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
static char __fastcall Detour_DoCulling(void* a1, void* a2, void* a3) {
    if(cvr::stereo::preparation::Active() && CyberpunkVR_CullReuseMode==0 && a2) {
        int side=-1;
        __try {auto* view=*reinterpret_cast<uint8_t**>(reinterpret_cast<uint8_t*>(a2)+0x18);
            if(view){if(*reinterpret_cast<uint64_t*>(view+0x28)==g_vrcam_ctx_key)side=1;else if(is_main_view(view))side=0;}}
        __except(EXCEPTION_EXECUTE_HANDLER){}
        cvr::stereo::preparation::Cull(a2,a3,side,false,a1);const auto result=g_orig_doculling(a1,a2,a3);
        cvr::stereo::preparation::Cull(a2,a3,side,true,a1);return result;
    }
    // mode6: GraphContextReset was skipped only on the VRCAM->MAIN transition, so MAIN
    // sees VRCAM's complete current-frame cull output and can skip its duplicate cull.
    if (CyberpunkVR_CullReuseMode == 6 && a2) {
        __try {
            uint8_t* view = *reinterpret_cast<uint8_t**>(reinterpret_cast<uint8_t*>(a2) + 0x18);
            if (view) {
                const uint64_t key = *reinterpret_cast<uint64_t*>(view + 0x28);
                const float aspect = *reinterpret_cast<float*>(view + 0x98);
                if (key == g_vrcam_ctx_key) {
                    g_visibility_reuse_phase.store(
                        VIS_REUSE_VRCAM_ACTIVE, std::memory_order_release);
                }
                if (is_main_view(view) &&
                    g_main_visibility_reuse_armed.load(std::memory_order_acquire) &&
                    g_visibility_reuse_phase.load(std::memory_order_acquire) == VIS_REUSE_MAIN) {
                    CyberpunkVR_DebugMainContainer = reinterpret_cast<uintptr_t>(
                        *reinterpret_cast<void**>(view + 0x2C40));
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugMainCullReuseHits);
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugCullSkipHits);
                    return 0;
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    if (doculling_is_vrcam(a2)) { CVR_DIAGNOSTIC(++CyberpunkVR_DebugCullSkipHits); return 0; }
    return g_orig_doculling(a1, a2, a3);
}
#endif

// --- mode7: replay compact VRCAM visibility into fresh MAIN output ---------
// sub_14029CCEC classifies global candidate records via AABB/frustum + optional HZB and
// emits batches of up to 32 tagged pointers: candidate_record | {1=intersect, 2=inside}.
// sub_1404ED29C copies those tags into an owned worker task and materializes against the
// CURRENT view context. Cache VRCAM's tags, then let MAIN run normal DoCulling + 62463C
// (fresh frame-local payload/setup) while replacing only 624694's tester-loop with replay.
static_assert(offsetof(ReplayVisibilityBatch, count) == 0x100);

#if !CVR_RUST_VRIK
using MainCullTestFn = __int64(__fastcall*)(void*, void*, void*, void*, void*);
#endif
#if !CVR_RUST_VRIK
using MainCullPrepareFn = __int64(__fastcall*)(void*, void*, void*);
using MainCullCtxInitFn = void*(__fastcall*)(void*, uintptr_t, void*, uint8_t);  // 2.5 added the bool (ctx+0x32 bit 3)
using VisibilityCollectorFn = __int64(__fastcall*)(void*, void*);
#endif
#if !CVR_RUST_VRIK
using MaterializeWorkerFn = void*(__fastcall*)(void*, void*);
using FineMaterializeFn = char(__fastcall*)(void*, __int64*, char, void*);
#endif
#if !CVR_RUST_VRIK
using VisibleAppendFn = char(__fastcall*)(void*, uintptr_t*);
#endif
uint64_t prepare_mix64(uint64_t value);
#if !CVR_RUST_VRIK
MainCullPrepareFn g_orig_main_cull_prepare = nullptr;
#endif
#if !CVR_RUST_VRIK
MainCullCtxInitFn g_main_cull_ctx_init = nullptr;
#endif
#if !CVR_RUST_VRIK
MainCullTestFn g_orig_main_cull_test = nullptr;
#endif
#if !CVR_RUST_VRIK
VisibilityCollectorFn g_orig_visibility_collector = nullptr;
#endif
#if !CVR_RUST_VRIK
static MaterializeWorkerFn g_orig_materialize_worker = nullptr;
#endif
#if !CVR_RUST_VRIK
FineMaterializeFn g_orig_fine_materialize = nullptr;
#endif
#if !CVR_RUST_VRIK
VisibleAppendFn g_orig_visible_append = nullptr;
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
thread_local bool t_capture_vrcam_visibility = false;
thread_local std::vector<ReplayVisibilityBatch> t_vrcam_visibility_batches;
std::mutex g_visibility_batches_mutex;
std::vector<ReplayVisibilityBatch> g_vrcam_visibility_batches;
std::atomic<uint64_t> g_vrcam_visibility_generation{0};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/stereo_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVisibilityBatchCaptures = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVisibilityBatchReplays = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVisibilityCandidatesCaptured = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVisibilityCandidatesReplayed = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainCullPrepareSkips = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CullCallbackProfileEnable = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullCallbackMethodRva[CULL_CALLBACK_MAX] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullCallbackCallsMain[CULL_CALLBACK_MAX] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullCallbackCallsVrcam[CULL_CALLBACK_MAX] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullCallbackDescMain[CULL_CALLBACK_MAX] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullCallbackDescVrcam[CULL_CALLBACK_MAX] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullCallbackTicksMain[CULL_CALLBACK_MAX] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCullCallbackTicksVrcam[CULL_CALLBACK_MAX] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFineCandidateCaptures = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFineCandidateReplays = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFineCandidateFallbacks = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFineDrawableIdsCaptured = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFineDrawableIdsReplayed = 0;
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
std::atomic<uint32_t> g_fine_reuse_phase{FINE_REUSE_IDLE};
std::mutex g_fine_visibility_mutex;
std::unordered_map<uintptr_t, std::vector<uintptr_t>> g_fine_visible_ids;
thread_local bool t_capture_fine_ids = false;
thread_local std::vector<uintptr_t> t_fine_ids;
static thread_local uint32_t t_materialize_view_kind = 0; // 1=VRCAM, 2=MAIN gameplay
thread_local uintptr_t t_materialize_output_key = 0;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/stereo_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_MaterializeProfileEnable = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeWorkerTasksMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeWorkerTasksVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeTaggedMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeTaggedVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeFineCallsMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeFineCallsVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeFineRangesMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeFineRangesVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeDrawableAppendsMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeDrawableAppendsVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeRangeUniqueMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeRangeUniqueVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeRangeDuplicateMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeRangeDuplicateVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeRangeProbeOverflowMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeRangeProbeOverflowVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeRangeCtxUniqueMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeRangeCtxUniqueVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeRangeCtxDuplicateMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeRangeCtxDuplicateVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeRangeCtxProbeOverflowMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaterializeRangeCtxProbeOverflowVrcam = 0;
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
constexpr uint32_t MATERIALIZE_RANGE_HASH_CAP = 1u << 22; // 4M slots, diagnostic only
static std::atomic<uint64_t> g_materialize_range_hash_main[MATERIALIZE_RANGE_HASH_CAP];
static std::atomic<uint64_t> g_materialize_range_hash_vrcam[MATERIALIZE_RANGE_HASH_CAP];
static std::atomic<uint64_t> g_materialize_range_ctx_hash_main[MATERIALIZE_RANGE_HASH_CAP];
static std::atomic<uint64_t> g_materialize_range_ctx_hash_vrcam[MATERIALIZE_RANGE_HASH_CAP];

void materialize_prof_add(uint64_t& main_value, uint64_t& vrcam_value, uint64_t value) {
    if (t_materialize_view_kind == 1)
        InterlockedAdd64(reinterpret_cast<volatile LONG64*>(&vrcam_value), value);
    else if (t_materialize_view_kind == 2)
        InterlockedAdd64(reinterpret_cast<volatile LONG64*>(&main_value), value);
}

void materialize_range_observe(uint64_t key_hash) {
    if (!t_materialize_view_kind || !key_hash)
        return;
    auto* table = t_materialize_view_kind == 1 ? g_materialize_range_hash_vrcam : g_materialize_range_hash_main;
    uint64_t& unique_ctr = t_materialize_view_kind == 1
        ? CyberpunkVR_DebugMaterializeRangeUniqueVrcam : CyberpunkVR_DebugMaterializeRangeUniqueMain;
    uint64_t& dup_ctr = t_materialize_view_kind == 1
        ? CyberpunkVR_DebugMaterializeRangeDuplicateVrcam : CyberpunkVR_DebugMaterializeRangeDuplicateMain;
    uint64_t& overflow_ctr = t_materialize_view_kind == 1
        ? CyberpunkVR_DebugMaterializeRangeProbeOverflowVrcam : CyberpunkVR_DebugMaterializeRangeProbeOverflowMain;
    const uint32_t start = static_cast<uint32_t>(key_hash) & (MATERIALIZE_RANGE_HASH_CAP - 1);
    for (uint32_t probe = 0; probe < 16; ++probe) {
        auto& slot = table[(start + probe) & (MATERIALIZE_RANGE_HASH_CAP - 1)];
        uint64_t cur = slot.load(std::memory_order_acquire);
        if (cur == key_hash) {
            ++dup_ctr;
            return;
        }
        if (cur == 0 && slot.compare_exchange_strong(cur, key_hash,
                std::memory_order_acq_rel, std::memory_order_acquire)) {
            ++unique_ctr;
            return;
        }
    }
    ++overflow_ctr;
}

void materialize_range_ctx_observe(uint64_t key_hash) {
    if (!t_materialize_view_kind || !key_hash)
        return;
    auto* table = t_materialize_view_kind == 1 ? g_materialize_range_ctx_hash_vrcam : g_materialize_range_ctx_hash_main;
    uint64_t& unique_ctr = t_materialize_view_kind == 1
        ? CyberpunkVR_DebugMaterializeRangeCtxUniqueVrcam : CyberpunkVR_DebugMaterializeRangeCtxUniqueMain;
    uint64_t& dup_ctr = t_materialize_view_kind == 1
        ? CyberpunkVR_DebugMaterializeRangeCtxDuplicateVrcam : CyberpunkVR_DebugMaterializeRangeCtxDuplicateMain;
    uint64_t& overflow_ctr = t_materialize_view_kind == 1
        ? CyberpunkVR_DebugMaterializeRangeCtxProbeOverflowVrcam : CyberpunkVR_DebugMaterializeRangeCtxProbeOverflowMain;
    const uint32_t start = static_cast<uint32_t>(key_hash) & (MATERIALIZE_RANGE_HASH_CAP - 1);
    for (uint32_t probe = 0; probe < 16; ++probe) {
        auto& slot = table[(start + probe) & (MATERIALIZE_RANGE_HASH_CAP - 1)];
        uint64_t cur = slot.load(std::memory_order_acquire);
        if (cur == key_hash) {
            ++dup_ctr;
            return;
        }
        if (cur == 0 && slot.compare_exchange_strong(cur, key_hash,
                std::memory_order_acq_rel, std::memory_order_acquire)) {
            ++unique_ctr;
            return;
        }
    }
    ++overflow_ctr;
}

static void* __fastcall Detour_MaterializeWorker(void* task, void* queue_ctx) {
    if(cvr::stereo::preparation::Active()){
        int side=-1;
        __try {auto* view=task?*reinterpret_cast<uint8_t**>(static_cast<uint8_t*>(task)+24):nullptr;
            if(view){if(*reinterpret_cast<uint64_t*>(view+0x28)==g_vrcam_ctx_key)side=1;else if(is_main_view(view))side=0;}}
        __except(EXCEPTION_EXECUTE_HANDLER){}
        const auto ticket=cvr::stereo::preparation::BeginMaterialize(task,side);
        void* const result=g_orig_materialize_worker(task,queue_ctx);
        cvr::stereo::preparation::EndMaterialize(ticket);return result;
    }
    if (!CyberpunkVR_MaterializeProfileEnable || !task)
        return g_orig_materialize_worker(task, queue_ctx);
    const uint32_t previous_kind = t_materialize_view_kind;
    const uintptr_t previous_output_key = t_materialize_output_key;
    t_materialize_view_kind = 0;
    t_materialize_output_key = 0;
    __try {
        auto* const view = *reinterpret_cast<uint8_t**>(reinterpret_cast<uint8_t*>(task) + 24);
        if (view) {
            const uint64_t key = *reinterpret_cast<uint64_t*>(view + 0x28);
            if (key == g_vrcam_ctx_key)
                t_materialize_view_kind = 1;
            else if (is_main_view(view))
                t_materialize_view_kind = 2;
        }
        t_materialize_output_key = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(task) + 80);
        const uint32_t tagged = *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(task) + 352);
        materialize_prof_add(CyberpunkVR_DebugMaterializeWorkerTasksMain,
            CyberpunkVR_DebugMaterializeWorkerTasksVrcam, 1);
        materialize_prof_add(CyberpunkVR_DebugMaterializeTaggedMain,
            CyberpunkVR_DebugMaterializeTaggedVrcam, tagged);
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    void* const result = g_orig_materialize_worker(task, queue_ctx);
    t_materialize_view_kind = previous_kind;
    t_materialize_output_key = previous_output_key;
    return result;
}
#endif

// ---- LOD/detail threshold sweep: override gather-context ctx+0x28 (sub_140623FD8) ----------
// 623FD8 builds the shared gather-context; ctx+0x28 is the LOD/detail threshold (normal 1.0,
// forced 0.1 for view-kind 6/7). It flows into scene-node gather methods (vfn+248/+320/+376),
// NOT into the 14DFE8 fine test. This override sweeps the threshold live so we can measure the
// DrawableAppends delta per view. a1=ctx out, a2=view ptr, a3=cull-query. key@view+0x28,
// aspect@view+0x98. Same detection ABI as the rest of the cull hooks.
#if !CVR_RUST_VRIK
MainCullCtxInitFn g_orig_gather_ctx_init = nullptr;
#endif
// OBSERVE-ONLY DEFAULT (2026-07-31): Enable=1 with ApplyMask=0 reads the threshold each view
// actually gets and writes nothing. Two night symptoms -- no stars, and distance lost to fog
// while MAIN is fine -- are both "a distant drawable is not there", and the note above says the
// engine FORCES this threshold to 0.1 for some view kinds instead of 1.0. If VRCAM is one of
// them it culls far geometry hard, which is exactly both symptoms. Set ApplyMask=2 and
// LodThreshValue to MAIN's reading to hand VRCAM the same threshold -- live, no rebuild.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_LodThreshOverrideEnable = 1;   // answered: detail threshold is 1.0 on both views
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/stereo_moved_exports.rs.
#else
extern "C" __declspec(dllexport) float    CyberpunkVR_LodThreshValue = 1.0f;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_LodThreshApplyMask = 0; // b0=MAIN b1=VRCAM b2=other
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLodThreshHitsMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLodThreshHitsVrcam = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugLodThreshHitsOther = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugLodThreshSeenMainBits = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugLodThreshSeenVrcamBits = 0;
#endif

// One line every 5 s: what threshold each view is handed. Reported as a float because that is
// what the field is -- 1.0 is "keep everything", the smaller it gets the earlier detail drops.
#if CVR_RUST_VRIK
#else
void lod_thresh_report() {
    static std::atomic<uint64_t> s_next{0};
    const uint64_t now = GetTickCount64();
    uint64_t due = s_next.load(std::memory_order_relaxed);
    if (now < due) return;
    if (!s_next.compare_exchange_strong(due, now + 5000, std::memory_order_relaxed)) return;
    const uint32_t bm = CyberpunkVR_DebugLodThreshSeenMainBits;
    const uint32_t bv = CyberpunkVR_DebugLodThreshSeenVrcamBits;
    float fm = 0.0f, fv = 0.0f;
    memcpy(&fm, &bm, 4);
    memcpy(&fv, &bv, 4);
    log("[lod] gather-ctx+0x28 detail threshold -- MAIN %.4f (0x%08X)  VRCAM %.4f (0x%08X)  "
        "applyMask=%u value=%.4f  hits M/V/other %llu/%llu/%llu",
        fm, bm, fv, bv, CyberpunkVR_LodThreshApplyMask, CyberpunkVR_LodThreshValue,
        CyberpunkVR_DebugLodThreshHitsMain, CyberpunkVR_DebugLodThreshHitsVrcam,
        CyberpunkVR_DebugLodThreshHitsOther);
}
#endif

// ---- registered where they are defined -----------------------------------------------------
#if CVR_RUST_VRIK
// THE CULLING HALF IS RUST'S (rust/crates/cvr-render/src/cull_visibility.rs): the detail threshold, the occlusion gate,
// the graph context's preparation and reset, DoCulling and the multifrustum worker, registered here with this file's
// trampolines; Rust is handed them, the main and second views' contexts and the stereo log (it calls is_main_view,
// NativePreparation and NativeCpuCollector's Rust halves itself).
// Its table is built in Rust (cull_visibility.rs, from stereo_shared.rs's and moved_globals.rs's objects).
// DoCulling and GraphContextPrepare register from Rust (rust/crates/cvr-render/src/engine_detours.rs).
#else
CVR_DETOUR_IF("[cull] DoCulling sub_140BF436C", DOCULLING_RVA, Detour_DoCulling, g_orig_doculling, cvr::engine_stereo::SecondViewWanted)
CVR_DETOUR("[cull] GraphContextPrepare sub_140423180", GRAPH_CONTEXT_PREPARE_RVA, Detour_GraphContextPrepare, g_orig_graph_context_prepare)
CVR_DETOUR_IF("[cull] GraphContextReset sub_1404ECEA0", GRAPH_CONTEXT_RESET_RVA, Detour_GraphContextReset, g_orig_graph_context_reset, cvr::engine_stereo::SecondViewWanted)
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/cull_visibility.rs
CVR_DETOUR_IF("[cull] MaterializeWorker sub_1404ED62C", MATERIALIZE_WORKER_RVA, Detour_MaterializeWorker, g_orig_materialize_worker, cvr::engine_stereo::SecondViewWanted)
#endif

#if CVR_RUST_VRIK
// THE PREPARATION AND REPLAY DETOURS ARE RUST'S TOO (cull_visibility.rs): the stage with its eager depth, tickets,
// profiler and cache audit, the cull callbacks' profile, the visibility capture and replay, the fine materialize and
// the materialize worker; Rust is handed their trampolines (NativePreparation's tickets and NativeCpuCollector's capture
// are its own).
static_assert(sizeof(cvr::stereo::preparation::Ticket) == 136, "the Ticket rust/crates/cvr-render/src/cull_visibility.rs keeps");
static_assert(sizeof(cvr::stereo::preparation::MaterializeTicket) == 120, "the MaterializeTicket cull_visibility.rs keeps");
// FineMaterialize and MainCullTest register from Rust (rust/crates/cvr-render/src/engine_detours.rs).
#endif
}  // namespace detail
}  // namespace cvr
#endif
