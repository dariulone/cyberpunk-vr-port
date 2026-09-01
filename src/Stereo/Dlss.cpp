// Dlss -- Streamline and DLSS, made to agree about two views.
//
// Five detours: the Streamline constant upload, the DLSS constants, the evaluate call, the apply, and
// the render-resolution query. Every one of them was written for a single view, and the second eye
// either has to be given its own answer or excluded from the first's -- getting that wrong is how the
// second view came out cropped, and how it came out washed out without DLSS.
//
// One of these writes view+0xF94, which the sky node ALSO reads as its own input. That coupling is
// noted where it is written; it is the reason "our own fix is an input to somebody else's decision"
// appears in this module's history at all.

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

static __int64 __fastcall Detour_SlConstants(void* a1, void* a2, void* a3) {
    // MAIN identity, step 2: this writer is the only place that sees the view OBJECT and the
    // view CTX for the same view, so it is where the object recorded from MAIN-only nodes
    // becomes a ctx pointer every other hook can compare against. Re-pinned every frame, so a
    // ctx pool that recycles pointers cannot leave a stale MAIN behind for more than a frame.
    // key == 0 is REQUIRED, not a nicety: without it this pinned VRCAM's ctx as MAIN (the
    // object the work-context vtable hands back is not per-view enough to separate them),
    // after which is_main_view() answered true for VRCAM and its whole branch went dead.
    if (a2) {
        __try {
            const uintptr_t want = g_main_view_obj.load(std::memory_order_acquire);
            if (want) {
                const uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
                    reinterpret_cast<uint8_t*>(a2) + 0x18);
                if (ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == 0 &&
                    sl_view_obj(a2) == want &&
                    g_main_view_ctx.exchange(ctx, std::memory_order_release) != ctx) {
                    ++CyberpunkVR_DebugMainCtxBinds;
                    CyberpunkVR_DebugMainCtx = static_cast<uint64_t>(ctx);
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    if (CyberpunkVR_StreamlineHistoryFix && a2) {
        __try {
            uintptr_t ctx = *reinterpret_cast<uintptr_t*>(
                reinterpret_cast<uint8_t*>(a2) + 0x18);
            if (ctx) {
                uint64_t key = *reinterpret_cast<uint64_t*>(ctx + 0x28);
                uintptr_t obj = sl_view_obj(a2);
                if (obj) {
                    uint32_t* mode = reinterpret_cast<uint32_t*>(obj + 0xF94);
                    const uint32_t build_mode =
                        *reinterpret_cast<uint32_t*>(obj + 0xF90);
                    if (key == g_vrcam_ctx_key) {
                        CyberpunkVR_DebugVrcamAaMode = *mode;
                        CyberpunkVR_DebugVrcamBuildModeF90 = build_mode;
                        uint32_t want = CyberpunkVR_DebugMainAaMode;   // mirror main
                        if (*mode != want) { *mode = want; ++CyberpunkVR_DebugSlHistoryHits; }
                    } else if (key == 0) {
                        CyberpunkVR_DebugMainAaMode = *mode;           // observe main
                        CyberpunkVR_DebugMainBuildModeF90 = build_mode;
                    }
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }

    // ---- per-eye IPD stereo + tripod + vrcam mirror (all independent, default OFF) ----
    // choice A: main = LEFT (-IPD/2), vrcam = RIGHT (+IPD/2). Applied BEFORE g_orig and
    // NOT restored so the whole frame (incl. the once-per-frame prev-camera capture) sees
    // the same camera -> motion vectors stay consistent (no shimmer).
    // Force vrcam camera fov + weapon ZOOM = MAIN via the CAMERA ctx (this writer runs
    // many times per frame -> smooth, exactly like the IPD transform above). Capture
    // MAIN (slot 0), apply to vrcam (slot 1). Same ctx: fov@0x90, zoom@0x9C. Orientation
    // (@0x80/0xC0) is left to the engine; aspect stays vrcam's own.
    if (CyberpunkVR_ForceVrcamCam && a2) {
        __try {
            uintptr_t ctx = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(a2) + 0x18);
            if (ctx) {
                const uint64_t key = *reinterpret_cast<uint64_t*>(ctx + 0x28);
                float* c = reinterpret_cast<float*>(ctx);
                if (is_main_view(reinterpret_cast<void*>(ctx))) {
                    g_main_cam_fov  = c[0x90 / 4];
                    g_main_cam_zoom = c[0x9C / 4];
                    g_main_cam_near = c[0xB0 / 4];
                    g_main_cam_far  = c[0xB4 / 4];
                    g_main_proj_yy  = c[0x214 / 4];
                    // Weapon ADS as a plain scale factor. MAIN's fov scalar is provably NOT
                    // touched by ADS (measured: 68.238 both at rest and while aiming), so it
                    // still describes the UNZOOMED frustum -- which makes cot(fovV/2) the
                    // baseline the live vertical scale divides by. No guessed reference.
                    // Measured: at rest 1.47593 * tan(34.119deg) = 1.0000; aiming 2.21332 the
                    // same way = 1.4998.
                    if (g_main_cam_fov > 0.f) {
                        const float t = tanf(g_main_cam_fov * 0.5f * 0.01745329252f);
                        if (t > 0.f) {
                            g_ads_factor = g_main_proj_yy * t;
                            CyberpunkVR_MainAdsZoomFactor = g_ads_factor;
                        }
                    }
                    CyberpunkVR_DebugMainCamFov = g_main_cam_fov;
                    CyberpunkVR_DebugMainProjYY = g_main_proj_yy;
                } else if (key == g_vrcam_ctx_key && g_main_cam_fov > 0.f) {
                    // ctx scalars: drive vrcam culling/LOD to match main (screen-space).
                    // ONE FIELD AT A TIME. The context steers culling; the component's own fov
                    // steers what is actually rendered. Handing over near/far makes the light
                    // cluster grid bin along MAIN's depth slicing while this view's pixels look
                    // themselves up along their own -- see the note by CyberpunkVR_VrcamCamFields.
                    const uint32_t camf = CyberpunkVR_VrcamCamFields;
                    if (camf & 1u) c[0x90 / 4] = g_main_cam_fov;    // fov
                    if (camf & 2u) c[0x9C / 4] = g_main_cam_zoom;   // zoom
                    if (camf & 4u) c[0xB0 / 4] = g_main_cam_near;   // near
                    if (camf & 8u) c[0xB4 / 4] = g_main_cam_far;    // far

                    // ---- VRCAM vertical FOV: the only input the RTT projection has --------
                    // Established by measurement (engine_re/dumps/F_rtt_camera_fov.md,
                    // G_rtt_zoom_consumer*.md, H_rtt_zoom_field.md):
                    //   * the projection is built from the component's fov at comp+0x128 and
                    //     nothing else -- the producer's source struct holds fov/aspect/
                    //     near/far and cot(68.238/2) == 1.47593 reproduces it exactly;
                    //   * the RTTI `zoom` field at +0x15C is never read on this path;
                    //   * the zoom ratio at +0x424 is an OUTPUT of the per-view setup;
                    //   * writing the projection into the view ctx steers CULLING only;
                    //   * that producer runs EVERY frame, standing still included, so the fov
                    //     write is sufficient on its own. Nothing needs re-invoking -- forcing
                    //     comp+0xA00 or calling sub_140AC316C drags view-create in, which
                    //     hitched the game and hung the GPU (DXGI_ERROR_DEVICE_HUNG).
                    // Computed in double: the value round-trips through the engine as
                    // fov -> cot -> projection, and doing the trig in float left ~0.002 deg
                    // of drift against the authored value.
                    double src_fov = 0.0;              // vertical FOV in degrees, pre-ADS
                    double ads = static_cast<double>(g_ads_factor);
                    if (CyberpunkVR_VrcamFovDeg > 1.0f) {
                        // Explicit override -- this is where the headset's own FOV goes once
                        // the HMD drives the eye. ADS still applies on top of it.
                        src_fov = static_cast<double>(CyberpunkVR_VrcamFovDeg);
                    } else if (g_main_proj_yy > 0.f) {
                        // Follow MAIN. Its projection ALREADY carries the ADS zoom, so the
                        // factor must not be applied a second time.
                        src_fov = 2.0 * atan(1.0 / static_cast<double>(g_main_proj_yy)) *
                                  57.29577951308232;
                        ads = 1.0;
                    } else if (g_vrcam_base_fov > 0.f) {
                        src_fov = static_cast<double>(g_vrcam_base_fov);
                    }
                    const uintptr_t comp = g_vrcam_comp.load(std::memory_order_acquire);
                    if (comp && src_fov > 1.0 && ads > 0.0) {
                        const double want = (ads == 1.0)
                            ? src_fov
                            : 2.0 * atan(tan(src_fov * 0.5 * 0.017453292519943295) / ads) *
                              57.29577951308232;
                        if (want > 1.0 && want < 175.0) {
                            *reinterpret_cast<float*>(comp + 0x128) = static_cast<float>(want);
                            CyberpunkVR_DebugVrcamWantFov = static_cast<float>(want);
                        }
                    }
                    ++CyberpunkVR_DebugForceCamHits;
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }

    // DLSS-for-vrcam: mark this camera-writer call as vrcam so the constants driver
    // (sub_14078933C, called INSIDE g_orig) flips to vrcam's own SL viewport.
    const bool prev_sl_active = t_vrcam_sl_active;
    if (CyberpunkVR_VrcamDlss && a2) {
        __try {
            uintptr_t ctx = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(a2) + 0x18);
            t_vrcam_sl_active = ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key;
        } __except (EXCEPTION_EXECUTE_HANDLER) { t_vrcam_sl_active = false; }
    }
    __int64 sl_ret = g_orig_sl_const(a1, a2, a3);
    t_vrcam_sl_active = prev_sl_active;
    return sl_ret;
}

// DLSS constants driver (sub_14078933C -> slSetConstants). Flip to vrcam's SL
// viewport while the camera writer is processing the vrcam view.
static __int64 __fastcall Detour_DlssConst(void* a1, unsigned int a2) {
    bool flipped = false; int32_t saved = 0;
    if (CyberpunkVR_VrcamDlss && t_vrcam_sl_active && a1) {
        __try {
            int32_t* vp = reinterpret_cast<int32_t*>(reinterpret_cast<uint8_t*>(a1) + DLSS_VP_OFF);
            saved = *vp; *vp = CyberpunkVR_VrcamDlssViewport; flipped = true;
            // A/B: zero the jitter the const-setter (g_orig) is about to copy into sl::Constants.
            // The camera-writer filled it just before this call; zeroing here makes DLSS treat the
            // vrcam frame as un-jittered.
            if (CyberpunkVR_VrcamDlssZeroJitter) {
                *reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(a1) + DLSS_JITTER_OFF)     = 0.0f;
                *reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(a1) + DLSS_JITTER_OFF + 4) = 0.0f;
            }
            ++CyberpunkVR_DebugVrcamDlssConstHits;
        } __except (EXCEPTION_EXECUTE_HANDLER) { flipped = false; }
    }
    __int64 r = g_orig_dlss_const(a1, a2);
    if (flipped) {
        __try {
            *reinterpret_cast<int32_t*>(reinterpret_cast<uint8_t*>(a1) + DLSS_VP_OFF) = saved;
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return r;
}

// DLSS tag+eval driver (sub_141D4FDC0). a2 = view spec; key at *(*(a2+0x18)+0x28).
// Flip to vrcam's own SL viewport around the whole call (covers its internal
// slSetTag x N + slEvaluateFeature) so vrcam gets a distinct DLSS feature/history.
static void __fastcall Detour_DlssEval(void* a1, void* a2, int a3, int a4, int a5,
                                       int a6, int a7, int a8, int a9, int a10,
                                       int a11, int a12, int a13, int a14) {
    bool flipped = false; int32_t saved = 0; uintptr_t cache_addr = 0; uintptr_t vrcam_ctx = 0;
    static uint8_t s_main_cache[DLSS_CACHE_SZ];   // (render thread only; serialized by CS in g_orig)
    if (CyberpunkVR_VrcamDlss && a1 && a2) {
        __try {
            uintptr_t ctx = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(a2) + 0x18);
            if (ctx && *reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key) {
                vrcam_ctx = ctx;
                int32_t* vp = reinterpret_cast<int32_t*>(reinterpret_cast<uint8_t*>(a1) + DLSS_VP_OFF);
                saved = *vp; *vp = CyberpunkVR_VrcamDlssViewport;
                // swap vrcam's own changed-detection cache in (stops per-frame feature recreate)
                cache_addr = reinterpret_cast<uintptr_t>(a1) + DLSS_CACHE_OFF;
                memcpy(s_main_cache, reinterpret_cast<void*>(cache_addr), DLSS_CACHE_SZ);
                if (g_vrcam_dlss_cache_valid)
                    memcpy(reinterpret_cast<void*>(cache_addr), g_vrcam_dlss_cache, DLSS_CACHE_SZ);
                flipped = true;
                ++CyberpunkVR_DebugVrcamDlssEvalHits;
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { flipped = false; }
    }
    g_orig_dlss_eval(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14);
    if (flipped) {
        __try {
            // save vrcam's updated cache for next frame, restore main's cache into a1
            memcpy(g_vrcam_dlss_cache, reinterpret_cast<void*>(cache_addr), DLSS_CACHE_SZ);
            g_vrcam_dlss_cache_valid = true;
            memcpy(reinterpret_cast<void*>(cache_addr), s_main_cache, DLSS_CACHE_SZ);
            *reinterpret_cast<int32_t*>(reinterpret_cast<uint8_t*>(a1) + DLSS_VP_OFF) = saved;
            (void)vrcam_ctx;
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    // POST-DLSS CROP FIX: vrcam's DLSS eval just recorded on THIS command-list thread; mark the
    // thread as post-DLSS so the RSSetViewports/ScissorRects hooks upscale the vrcam blit's
    // render-res (1418) viewport to the output (2444). Set OUTSIDE the SEH block (POD write) and
    // only for the vrcam eval. Cleared at the command-list Reset that starts the next frame.
    if (flipped && CyberpunkVR_VrcamDlssScale)
        t_vrcam_dlss_post = true;
}

// Did MAIN's ApplyDLSS have flag 0x45 set (i.e. is the game actually using DLSS)? Observed
// each frame from main's ApplyDLSS call; vrcam only mirrors it when true. This makes the
// feature a strict MIRROR of main's upscaler: if the user has DLSS OFF (main lacks flag
// 0x45, or the ApplyDLSS node isn't even emitted), vrcam is never forced into DLSS.
static bool g_main_dlss_flag = false;

// ApplyDLSS node work-fn: mirror main's DLSS onto vrcam. For main (key 0) we OBSERVE flag
// 0x45; for vrcam we SET it (only if main has it) so vrcam takes the full eval path AND all
// POST-DLSS nodes (tonemap/bloom sub_140769308 read flags 0x45/0x47/0x48/0x49 to pick the
// DLSS-output source+dims). CRITICAL: main keeps 0x45 set the WHOLE frame; the tonemap is a
// SEPARATE node that runs after ApplyDLSS. Restoring 0x45 right after ApplyDLSS left vrcam's
// tonemap seeing 0x45=0 -> it read the wrong (pre-DLSS, since-aliased placed) source -> the
// top band flickered as that memory got reused. So set-and-KEEP it persistently (like main,
// same lesson as the IPD write); clear only when main drops DLSS or VrcamDlss is toggled off
// (so vrcam never gets stuck in a DLSS path with no eval behind it). NEVER forces DLSS on.
static __int64 __fastcall Detour_ApplyDlss(void* a1, void* a2) {
    if (a2) {
        __try {
            uintptr_t ctx = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(a2) + 0x18);
            if (ctx) {
                const uint64_t key = *reinterpret_cast<uint64_t*>(ctx + 0x28);
                const uintptr_t q = ctx + DLSS_FLAGSET_OFF;
                if (key == 0) {
                    g_main_dlss_flag = (*reinterpret_cast<uint64_t*>(q) & DLSS_EVAL_FLAG_BIT) != 0;
                } else if (key == g_vrcam_ctx_key) {
                    // want the flag set iff the feature is enabled AND main is actually on DLSS
                    const bool want = (CyberpunkVR_VrcamDlss != 0) && g_main_dlss_flag;
                    const uint64_t cur = *reinterpret_cast<uint64_t*>(q);
                    if (want && !(cur & DLSS_EVAL_FLAG_BIT))
                        *reinterpret_cast<uint64_t*>(q) = cur | DLSS_EVAL_FLAG_BIT;   // set & keep
                    else if (!want && (cur & DLSS_EVAL_FLAG_BIT))
                        *reinterpret_cast<uint64_t*>(q) = cur & ~DLSS_EVAL_FLAG_BIT;  // unstick
                    ++CyberpunkVR_DebugVrcamApplyDlssHits;
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return g_orig_applydlss(a1, a2);
}

// PATH-A graph-level experiment: skip the DLSS-gated salt70 post-color node for the
// vrcam view. Mirrors the node's own view/id computation: view = (a2[6]&1)?0:a2[13];
// id = (view<<24)^0x3D7E6258; vrcam id = 0x3C7E6258. When skipped we do NOT call the
// original, removing its type-4 post-color write (and its readback) from the vrcam graph.

// ===== VRCAM DLSS render-res downscale: make vrcam UPSCALE instead of DLAA =====
// Root cause of vrcam-DLAA (found via HW-write BP on the vrcam view's render-res field):
// sub_1404E42A0 computes each view's DLSS render resolution. Its a1 == &view[0x34] (the
// render-res sub-struct): renderW@+0, renderH@+4, prevW/H@+8/12, dupW@+16/+20, dupH@+24/+28,
// targetW@+32, targetH@+36, accum@+48. For the MAIN view the engine scales target x DLSS-scale
// (~0.58 Balanced, via renderer vtable+1080) because its DLSS flag (bit 0x20 @ view+0x17D8) is
// set; the VRCAM view's flag is NOT set yet at render-setup (ApplyDLSS sets it later in the
// frame) so vrcam falls through to render==target (1:1 == DLAA).
// FIX: after the engine computes vrcam's 1:1 res, overwrite it with round(target x scale) so
// vrcam's scene renders at ~58% into its 2444^2 RTs and DLSS upscales to 2444^2 (real FPS).
// This mirrors the engine's own scaled branch exactly (verified live: main 1920x1080 ->
// 1114x627 == x0.580175). Scale is read from the SHARED DLSS-state (renderer+0x4658)+0x400 ==
// the value main uses, so vrcam auto-mirrors main's quality mode (DLAA->skip, Balanced->0.58,
// Performance->0.5). Discriminator = view identity (CName "vrcam" @ view+0x28), NOT resolution.
// Gated by VrcamDlss (only when vrcam is on DLSS) + dedicated toggle CyberpunkVR_VrcamDlssScale.
using RenderResFn = __int64(__fastcall*)(void*, void*, void*);
static RenderResFn g_orig_render_res = nullptr;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamDlssScale         = 1;  // RETIRED/vestigial: core upscale + crop-fix now key off VrcamDlss ALONE (see Detour_RenderRes). FlagCompute drives the native downscale from VrcamDlss. Kept default=1 for overlay/back-compat (only gates dormant STAGE2 / diagnostics now).
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamClearFlag64       = 1;  // 1=clear vrcam build-flag 64 (bit0 view+0x17D8) -> build output-res post like main (crop fix)
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugClearFlag64Hits   = 0;  // times flag64 cleared for vrcam
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamResScaleHits = 0;  // times vrcam res was scaled
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugVrcamRenderW      = 0;  // last vrcam render width (diag)
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugVrcamRenderH      = 0;  // last vrcam render height (diag)
// COMPUTE-RESOLVE ROUTING (plan B, live-tunable): the vrcam-only crop = raster tonemap
// sub_140768510, gated by GROUP 20 = bit20 of view+0x17D0 (sub_14023AF5C(ctx,20)). LIVE-CONFIRMED:
// main bit20=0 (SKIPS the raster tonemap body -> composites via COMPUTE, res-agnostic, full output),
// vrcam bit20=1 (runs raster tonemap @ render-res 1418 viewport on the 2444 DLSS output -> CROP).
// The ONLY two view+0x17D0 bits that differ main-vs-vrcam are bit20 (main0/vrcam1) & bit25 (main1/vrcam0).
// Mode: 0=off, 1=clear bit20, 2=clear bit20 + set bit25 (== EXACT match to MAIN), 3=set bit25 only.
// Read at EXECUTE by the tonemap's own gate every frame => NOT the cached build-graph group-69 dead end.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamComputeResolve   = 2;  // SHIPPED DEFAULT=2. LIVE-VERDICT: bit20 (group 20) is necessary+sufficient for the crop (clear=no crop, set=crop; mode 3 proved bit25 alone irrelevant). 2 = EXACT match to MAIN's view+0x17D0 (a config main runs every frame => known-good, no synthetic hybrid). Active under VrcamDlss ALONE (VrcamDlssScale retired); DLAA/no-DLSS: matching main's flags is a no-op there.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugP17D0Hits        = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugP17D0Before      = 0;  // view+0x17D0 before our edit (diag)
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugP17D0After       = 0;  // view+0x17D0 after our edit (diag)

// ===== POST-DLSS CROP FIX: command-list viewport/scissor correction (slots 21/22/10) =====
// The vrcam-only post-DLSS blit (PipelineState_563) sets a render-res (1418) viewport+scissor on
// the 2444 DLSS-output RT -> top-left crop. We upscale that viewport/scissor to the vrcam output
// (2444) but ONLY: (a) this command-list thread is in the post-DLSS phase (t_vrcam_dlss_post, set
// at the vrcam DLSS eval on this same thread, cleared at the next Reset), and (b) the rect is
// exactly the vrcam RENDER size (main never uses the square vrcam render size, so main is never
// touched; pre-DLSS vrcam passes run before the eval so the phase flag is still false for them).
// CROP-PASS gate: viewport is the vrcam RENDER size (1418) while the RT bound on THIS thread is the
// vrcam OUTPUT size (2444) => the 563 blit under-filling the 2444 target. Thread-agnostic (t_cur_rt
// captured in hk_OMSetRenderTargets on the same recording thread just before this call).
// CROP-PASS gate (thread-independent, RTV-free): viewport width == the vrcam RENDER size (1418)
// while DLSS-upscale is active. Main never uses the square vrcam render size, so main is never
// matched. This catches BOTH pre-DLSS vrcam scene passes AND the post-DLSS crop pass; we separate
// them offline by intersecting the captured node RVAs with the readers of post-color 0x3D7E6258.
// (removed: vrcam_render_res_viewport helper -- only used by the now-pass-through viewport/scissor hooks)
void STDMETHODCALLTYPE hk_RSSetViewports(
        ID3D12GraphicsCommandList* self, UINT count, const D3D12_VIEWPORT* vps) {
    // Pass-through. The post-DLSS crop is fixed natively in Detour_RenderRes (view+0x17D0 match-main);
    // the old render-res-viewport band-aid + per-frame stack-capture diagnostics were removed.
    const CommandListVtableHook* e = command_list_hook_entry(self);
    PFN_RSSetViewports orig = e ? e->viewports_original : nullptr;
    if (orig) orig(self, count, vps);
}
 void STDMETHODCALLTYPE hk_RSSetScissorRects(
        ID3D12GraphicsCommandList* self, UINT count, const D3D12_RECT* rects) {
    // Pass-through (see hk_RSSetViewports).
    const CommandListVtableHook* e = command_list_hook_entry(self);
    PFN_RSSetScissorRects orig = e ? e->scissor_original : nullptr;
    if (orig) orig(self, count, rects);
}
 HRESULT STDMETHODCALLTYPE hk_GfxReset(
        ID3D12GraphicsCommandList* self, ID3D12CommandAllocator* alloc,
        ID3D12PipelineState* pso) {
    t_vrcam_dlss_post = false;
    t_cur_rt_w = 0; t_cur_rt_h = 0;   // stale RT cleared at frame-start recording
    // A reset ends the recording that owned any pending HUD bind. Dropping it here means the
    // snapshot can never barrier a resource whose render-target state belonged to a list that
    // no longer exists.
    t_hud_rt_bound = nullptr; t_hud_rt_list = nullptr;
    // A RESET ENDS THIS LIST'S IDENTITY, for the same reason as the HUD bind above. Whatever the
    // finished-frame path latched or learned on this list belonged to the previous recording; carrying
    // it over would let a list that MAIN now records into look like the second view's.
    if (t_final_list == self) { t_final_res = nullptr; t_final_list = nullptr; t_final_writes = 0; }
    const CommandListVtableHook* e = command_list_hook_entry(self);
    PFN_GfxReset orig = e ? e->reset_original : nullptr;
    return orig ? orig(self, alloc, pso) : S_OK;
}

// EXPERIMENT (attempt 2): overriding only the render-res struct was DISPROVEN live -- the
// whole struct (view+0x34..) went to 1418 yet the vrcam SCENE still rendered 2444 (DLSS then
// cropped the 1418 sub-rect of a full-2444 image => ZOOM). So the pass rasterizer viewport is
// NOT driven by the render-res value; it is gated by the DLSS/dynamic-res FLAG itself
// (bit 0x20 == flag 0x45 @ view+0x17D8). MAIN has that flag set for the whole frame (=> scaled
// render-res AND dynamic-res viewport); vrcam's is cleared by the per-frame view reset and only
// re-set LATE by ApplyDLSS. Attempt 2: set the flag EARLY (before g_orig, at the earliest
// per-frame setup hook we own) so vrcam takes main's full dynamic-res path. Still gated by the
// toggle (default OFF until proven); belt-and-suspenders render-res override kept for determinism.
static __int64 __fastcall Detour_RenderRes(void* a1, void* a2, void* a3) {
    bool vrcam = false;
    // CONSOLIDATED: gate the whole vrcam upscale + crop-fix path on VrcamDlss ALONE. VrcamDlssScale
    // is retired -- Detour_FlagCompute (VrcamDlss-gated) forces the DLSS upscaler group at graph
    // build, so the engine's render-res writer (g_orig below) downscales vrcam natively from VrcamDlss
    // alone; the separate "upscale" toggle is no longer needed. The downscale override below still
    // self-guards on main's actual DLSS scale (0.30..0.999), so DLAA / no-DLSS stay 1:1.
    if (CyberpunkVR_VrcamDlss && a1) {
        __try {
            uint8_t* view = reinterpret_cast<uint8_t*>(a1) - 0x34;   // a1 == &view[0x34]
            if (*reinterpret_cast<uint64_t*>(view + 0x28) == g_vrcam_ctx_key) {
                vrcam = true;
                // set the master DLSS/dynamic-res flag EARLY so g_orig scales AND the later
                // pass-viewport setup takes the dynamic-res path (like main), for this frame.
                *reinterpret_cast<uint64_t*>(view + DLSS_FLAGSET_OFF) |= DLSS_EVAL_FLAG_BIT;
                // ROOT-CAUSE FIX (build-time flag gate): flag 64 (bit0 of view+0x17D8) is the SOLE
                // main/vrcam view-flag difference (main=0, vrcam=1). The frame-graph SCENE_FULL
                // builder (sub_141D43040) tests it via sub_1407305B0(ctx+0x17D0, N) at BUILD time to
                // decide the post/final chain. Clearing it -> vrcam's graph is built like MAIN's
                // (output-res post declarations) while the scene still renders downscaled (below)
                // => native DLSS upscale, no crop. Detour_RenderRes runs pre-build so the clear is
                // seen by the builder. A/B via CyberpunkVR_VrcamClearFlag64.
                if (CyberpunkVR_VrcamClearFlag64) {
                    *reinterpret_cast<uint64_t*>(view + DLSS_FLAGSET_OFF) &= ~1ULL;
                    InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugClearFlag64Hits));
                }
                // PLAN B (live-tunable): match MAIN's view+0x17D0 group flags so vrcam SKIPS the
                // raster tonemap (group 20) and composites via the COMPUTE path like main -> no crop.
                if (CyberpunkVR_VrcamComputeResolve) {
                    uint64_t* p = reinterpret_cast<uint64_t*>(view + 0x17D0);
                    uint64_t before = *p, nv = before;
                    switch (CyberpunkVR_VrcamComputeResolve) {
                        case 1: nv &= ~(1ull << 20); break;                    // clear group 20
                        case 2: nv = (nv & ~(1ull << 20)) | (1ull << 25); break; // match MAIN exactly
                        case 3: nv |= (1ull << 25); break;                    // set group 25 only
                        default: break;
                    }
                    *p = nv;
                    CyberpunkVR_DebugP17D0Before = before;
                    CyberpunkVR_DebugP17D0After  = nv;
                    InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugP17D0Hits));
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { vrcam = false; }
    }
    __int64 r = g_orig_render_res(a1, a2, a3);
    if (vrcam && a1) {
        __try {
            float scale = 0.0f;
            uintptr_t renderer = *reinterpret_cast<uintptr_t*>(g_exe_base + RENDERER_GLOBAL_RVA);
            if (renderer) {
                uintptr_t dlss = *reinterpret_cast<uintptr_t*>(renderer + OFF_VIEWSTATE);
                if (dlss) scale = *reinterpret_cast<float*>(dlss + 0x400);
            }
            int32_t* p = reinterpret_cast<int32_t*>(a1);
            int32_t tW = p[8];    // a1+32 target W
            int32_t tH = p[9];    // a1+36 target H
            // only when main is actually upscaling (skip DLAA / insane values)
            if (scale > 0.30f && scale < 0.999f && tW > 0 && tH > 0) {
                int32_t rW = static_cast<int32_t>(static_cast<float>(tW) * scale + 0.5f);
                int32_t rH = static_cast<int32_t>(static_cast<float>(tH) * scale + 0.5f);
                if (rW < 1) rW = 1;
                if (rH < 1) rH = 1;
                p[0] = rW; p[4] = rW; p[5] = rW;    // renderW: a1+0, a1+16, a1+20
                p[1] = rH; p[6] = rH; p[7] = rH;    // renderH: a1+4, a1+24, a1+28
                *reinterpret_cast<uint64_t*>(reinterpret_cast<uint8_t*>(a1) + 48) = 0; // reset accum (engine scaled branch does this)
                CyberpunkVR_DebugVrcamRenderW = static_cast<uint32_t>(rW);
                CyberpunkVR_DebugVrcamRenderH = static_cast<uint32_t>(rH);
                // publish vrcam render+output dims for the DLSS const/eval subrect+MV fix
                g_vrcam_dlss_rw = rW; g_vrcam_dlss_rh = rH;
                g_vrcam_dlss_ow = tW; g_vrcam_dlss_oh = tH;
                InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugVrcamResScaleHits));
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return r;
}


// ---- registered here, in the file that defines them --------------------------------------------
CVR_DETOUR("[flagforce] flag-compute sub_141D49540", FLAG_COMPUTE_RVA, Detour_FlagCompute, g_orig_flag_compute)
CVR_DETOUR("[dlss] ApplyDLSS work sub_14037D5C4", APPLYDLSS_WORK_RVA, Detour_ApplyDlss, g_orig_applydlss)
CVR_DETOUR("[dlss] constants driver sub_14078933C", DLSS_CONST_RVA, Detour_DlssConst, g_orig_dlss_const)
CVR_DETOUR("[dlss] tag/eval driver sub_141D4FDC0", DLSS_EVAL_RVA, Detour_DlssEval, g_orig_dlss_eval)
CVR_DETOUR("[dlss] render-res scaler sub_1404E42A0", RENDER_RES_RVA, Detour_RenderRes, g_orig_render_res)
CVR_DETOUR("[sl] SetStreamlineConstants sub_140788A9C", SL_CONSTANTS_RVA, Detour_SlConstants, g_orig_sl_const)

// ================================================================================================
// THE VRCAM STREAMLINE VIEWPORT KNOBS, moved out of the monolith to sit with the DLSS path they steer.
//
// Giving vrcam its OWN Streamline viewport is what stops the two views sharing one set of DLSS history
// and jitter. These are the live controls for that: which viewport id, whether jitter is zeroed, and
// the render/output dimensions the second view reports.
// ================================================================================================

// ---- DLSS-for-VRCAM: give vrcam its OWN Streamline viewport ----------------
// The engine's DLSS drivers key off ONE global DLSS-state object (arg a1); the
// Streamline ViewportHandle is at a1+0x478 with the viewport id at a1+0x498 (=0
// for main). Both the constants driver (sub_14078933C -> slSetConstants) and the
// tag+eval driver (sub_141D4FDC0 -> slSetTag x N + slEvaluateFeature) submit to
// that single viewport, so vrcam (which already reaches the eval driver with its
// OWN ctx + resources -- verified live: eval a2 key == VRCAM) collides on main's
// viewport 0 and produces no distinct DLSS. Fix: when the current view is vrcam,
// flip a1+0x498 to a distinct id (default 1) around each driver call and restore
// after (idempotent, no persistent state). Streamline auto-creates a 2nd DLSS
// feature (own temporal history) for the new viewport on first eval. RVAs @ base
// 0x7FF6EF660000 (found via named refs to sl.interposer exports).
using DlssEvalFn  = void(__fastcall*)(void*, void*, int, int, int, int, int,
                                      int, int, int, int, int, int, int);
using DlssConstFn = __int64(__fastcall*)(void*, unsigned int);
DlssEvalFn  g_orig_dlss_eval  = nullptr;
DlssConstFn g_orig_dlss_const = nullptr;
// PATH-A graph-level EXPERIMENT (doc 19): the DLSS-gated frame-graph node
// sub_14292DD50 (vtable 0x14312CDA8 +0x28) inserts an extra type-4 WRITE declare of
// post-color at salt70 and does a GPU->CPU readback. It perturbs the salt70 post-color
// generation timeline so Final2D's fixed read flaps between the correct (dark) and DLSS
// (bright) versions. This flag skips the node FOR THE VRCAM VIEW ONLY, to test whether
// removing it from the salt70 timeline makes Final2D deterministically read the correct
// version. DEFAULT 0 (no effect on shipped build); reversible; SEH-guarded.
using ReadbackNodeFn = char(__fastcall*)(__int64, void*);
static ReadbackNodeFn g_orig_readback_node = nullptr;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_SkipVrcamReadbackNode = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugReadbackSkips = 0;
// AUTOMATIC, NOT A SETTING. Written by Detour_FlagCompute from MAIN's own upscaler groups and
// read everywhere else; there is no overlay switch any more.
//
// It was never a real choice. Every gate that consumes it already refused to act unless MAIN had
// group 69 set, so the user-facing switch was only ever the redundant half of an AND -- and the
// half that could be wrong. On it does two things, both of which only make sense while MAIN is
// upscaling: it stands vrcam up in its own Streamline viewport, and it makes vrcam render below
// its target and upscale rather than run DLAA.
//
// Without the separate viewport the two views share viewport 0, which is not merely "vrcam gets
// no DLSS of its own" -- it actively corrupts MAIN. Both views push camera matrices and jitter
// into the same viewport and evaluate against the same temporal history, so MAIN's history is
// interleaved with frames from a camera that is somewhere else. DLSS then resolves against a
// history that keeps jumping, which reads as distant geometry shimmering under head rotation, and
// it disappears entirely with the VRCAM component off, because then nothing else touches
// viewport 0. So when MAIN is on DLSS, this being on is the correct state, not an opt-in.
//
// 0 while MAIN is not upscaling (DLAA, TAA, no upscaler, or before the first graph build) --
// which is exactly the old default, so nothing changes for those setups.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_VrcamDlss          = 0;  // derived; do not set by hand
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_VrcamDlssViewport  = 1;  // distinct SL viewport id
// DIAGNOSTIC A/B: zero the DLSS jitter the engine feeds vrcam (live float @ a1+0x1E0/+0x1E4,
// found in x64dbg -- the source the const-setter copies into sl::Constants). If vrcam's
// GEOMETRY render is NOT jittered but DLSS un-jitters by this offset, that mismatch shimmers;
// forcing jitter=0 aligns them (stable spatial DLAA) and the flicker should stop. If the
// render IS jittered, zeroing makes it worse -> tells us jitter is not the cause.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_VrcamDlssZeroJitter = 0;  // 1=force vrcam DLSS jitter to 0
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamDlssEvalHits  = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamDlssConstHits = 0;
// Set by Detour_SlConstants (camera writer) while it runs for the vrcam view, so
// the constants driver -- called INSIDE the camera writer -- knows to flip too.
thread_local bool t_vrcam_sl_active = false;

// ApplyDLSS node work-fn (sub_14037D5C4). vrcam takes a NO-EVAL path because it lacks
// feature flag 0x45 ("DLSS eval enable"): the flag bitset is at *(ctx+0x18)+0x17D0, so
// flag 0x45 = bit (0x45&0x3F=5) of the qword at +0x17D0+(0x45>>6)*8 = +0x17D8. main has
// it set, vrcam does not (VrcamFlagMode only mirrors the FG f0/f1 word, a different set).
// Setting it for vrcam makes ApplyDLSS take the FULL eval path -> reaches the eval driver
// (verified live SAFE: reaches eval, no crash -- unlike forcing the owner bit at ctx+0x30&2
// which runs an owner-only block reading [ctx+0x1D70]+0x268 that vrcam lacks). Set before
// g_orig, restore after (minimal footprint).
using ApplyDlssFn = __int64(__fastcall*)(void*, void*);
ApplyDlssFn g_orig_applydlss = nullptr;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamApplyDlssHits = 0;

//  VRCAM POST -> CASE C (global output res) FORCE 
// Verified live (x64dbg): the view-dims getter sub_1401EDA54 returns CASE A (render
// VP+0x34=1418) for vrcam post passes (callers 772BAC/61F6D4) because ctx+0x18(VP) &&
// ctx+0x20(DRS-gate = global DRS-scaler 0x1A32..) are set. Our forced DLSS flag keeps
// vrcam DRS-active the WHOLE frame -> every resource (scene+post) gets the gate -> CASE A.
// main's POST resources have the gate clear -> CASE C -> renderer+0x148 (global output).
// FIX: hook the getter ENTRY sub_1401ED8E4 (clean 5-byte prologue, unlike the tiny-leaf
// sub_1401EDA54); for vrcam (key) in the POST phase (t_vpost, set after ApplyDLSS, cleared
// at DeclCommon/scene start) clear ctx+0x20 (gate) + ctx+0x2C (node-local dims, to skip
// CASE B) around g_orig -> getter returns CASE C = global output. In VR global == HMD
// per-eye res == vrcam output -> post lands full-res, no crop. Scene (pre-DLSS, t_vpost=0)
// keeps CASE A (render 1418) so the DLSS upscale is preserved. Save+restore so only the
// getter sees the cleared desc (caller's struct intact).
using GetterFn = __int64(__fastcall*)(void*, void*);
static GetterFn g_orig_getter = nullptr;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamForceCaseC   = 0;  // vrcam post getter override: 0=off, 1=CASE C (renderer+0x148 global output; VR-correct but desktop=window res != 2444), 2=CASE B (vrcam OWN output VP+0x54=2444; matches RTT texture+DLSS out in BOTH desktop and VR -- preferred)
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugForceCaseCHits = 0;

// The eval driver's "feature changed?" check (in sub_141D4FDC0) compares the CURRENT view
// dims/ids against a cache stored in the single global DLSS-state a1 at a1+0x3C8..+0x3E7.
// With one shared state, the cache alternates between main's render res (e.g. 1114x627) and
// vrcam's (2444x2444) every frame -> "changed" always trips -> the per-viewport DLSS feature
// is RECREATED every frame -> vrcam's temporal history is reset every frame -> white flicker.
// Fix: keep a SEPARATE copy of that cache for vrcam and swap it in/out of a1 around vrcam's
// eval, so each viewport's "changed" check sees its OWN previous dims (constant) -> no false
// recreate -> stable history. (verified live: cache held main's 1114x627 while vrcam=2444.)
uint8_t g_vrcam_dlss_cache[DLSS_CACHE_SZ] = {0};
bool    g_vrcam_dlss_cache_valid = false;

// --- vrcam DLSS render/output dims (for the subrect+MV fix) --------------------
// Nsight proved: vrcam's OWN DLSS feature gets Render.Subrect.Dimensions and MV.Scale =
// 1114x627 (== MAIN's output 1920x1080 x scale), not vrcam's 1418x1418 (2444 x scale),
// because those derive from the SHARED DLSS-state render/output dim fields (a1+0x3D0 render,
// a1+0x3E0 output = main's), NOT from view+52/56. So DLSS reads a 1114x627 subrect of the
// square 1418^2 input -> crop. Fix: while vrcam's DLSS const/eval run, overwrite a1+0x3D0/0x3E0
// with vrcam's dims (render 1418, output 2444) so the subrect+MV become 1418. Values captured
// by Detour_RenderRes (runs earlier each frame). Gate: VrcamDlssScale (fwd-declared; defined below).
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamDlssScale;   // fwd decl (definition further down)
volatile int32_t g_vrcam_dlss_rw = 0, g_vrcam_dlss_rh = 0;  // vrcam render dims (1418)
volatile int32_t g_vrcam_dlss_ow = 0, g_vrcam_dlss_oh = 0;  // vrcam output dims (2444)

}  // namespace detail
}  // namespace cvr
