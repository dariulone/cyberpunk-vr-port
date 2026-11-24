#include "Hooks/ListAudit.hpp"
#include "Natives/MenuSurface.hpp"
#include "Utils/DebugGate.hpp"
#include "Render/SceneGpuProfile.hpp"
#include "Render/NativeStereoProbe.hpp"
#include "Render/NativeStereoFamilies.hpp"
#include "Render/NativeConstantCapture.hpp"
#include "Render/RegisteredConstantUpload.hpp"
#include "Render/NativeTextureRead.hpp"
#include "Render/NativeBufferRead.hpp"
#include "Render/NativeCommandRecording.hpp"
#include "Render/NativeRetainedGeometry.hpp"
#include "Render/NativeTargetAccess.hpp"
#include "Render/NativeMaterialBinding.hpp"
#include "Render/NativeSourceCbvBinding.hpp"
#include "Render/NativeSourceSrvBinding.hpp"
#include "Render/CommandResources.hpp"
#include "Render/FoveatedDlss.hpp"
#include "Render/FixedVrs.hpp"
#include "Render/PhysicalOpticsShaders.hpp"
#include "Render/MonoCascades.hpp"
#include "Render/NativePreparation.hpp"
#include "Stereo/SinglePass.hpp"
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
#include "Stereo/ScopeDlssState.hpp"
#include "Stereo/ScopeRrOptions.hpp"
#include "Stereo/ScopeResolution.hpp"
#include "Framegen/Inputs.hpp"
#include "Camera/ImagePoseIdentity.hpp"
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
#include <sl_core_types.h>
#include "MinHook.h"
#include "Utils/LogThrottle.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Stereo/StereoPairProbe.hpp"

namespace cvr {
namespace detail {

namespace {
scope_dlss::State scopeState;
thread_local bool scopeConstants = false;
thread_local uint8_t* scopeEvaluation = nullptr;

using RrOptionsFn=sl::Result(*)(const sl::ViewportHandle&,const sl::DLSSDOptions&);
RrOptionsFn originalRrOptions=nullptr;
static sl::Result ScopeRrOptions(const sl::ViewportHandle& viewport,const sl::DLSSDOptions& options) {
    auto* p=scopeEvaluation;
    if(!p || uint32_t(viewport)!=scope_dlss::kViewport ||
       options.structType!=sl::DLSSDOptions::s_structType || options.structVersion!=sl::kStructVersion3)
        return originalRrOptions(viewport,options);
    auto matched=options;
    if(!scope_dlss::MatchRrMode(uint32_t(viewport),matched,
        scope_dlss::Read<uint32_t>(p,0x3d0),scope_dlss::Read<uint32_t>(p,0x3d4),
        scope_dlss::Read<uint32_t>(p,0x3e0),scope_dlss::Read<uint32_t>(p,0x3e4)))
        return originalRrOptions(viewport,options);
    const auto result=originalRrOptions(viewport,matched);
    static unsigned reports=0;
    if(reports++<3) Log("[scope-dlss] RR 1:1 mode=%u->%u extent=%ux%u result=%d; MAIN options unchanged\n",
        unsigned(options.mode),unsigned(matched.mode),matched.outputWidth,matched.outputHeight,int(result));
    return result;
}
static bool RrOptionsSiteMatches() {
    // CP2.5 native wrapper of slDLSSDSetOptions, verified by its1001 feature import.
    const uint8_t expected[]{0x48,0x89,0x5c,0x24,0x08,0x57,0x48,0x83,0xec,0x20};
    return std::memcmp(g_exe_base+0x22902d0,expected,sizeof(expected))==0;
}

CRITICAL_SECTION* NativeDlssLock() {
    // Derive the lock used by BOTH native Evaluate and Configure, and validate
    // its EnterCriticalSection import before using it. The hook entry is earlier
    // than these bytes. Using a separate mutex would miss native settings writes.
    static auto* lock = []() -> CRITICAL_SECTION* {
        auto* site = g_exe_base + DLSS_EVAL_RVA + 0x21;
        auto* call = site + 13;
        if (site[0]!=0x48 || site[1]!=0x8d || site[2]!=0x0d ||
            call[0]!=0xff || call[1]!=0x15) return nullptr;
        const auto target = call+6+scope_dlss::Read<int32_t>(call,2);
        if (*reinterpret_cast<void**>(target)!=reinterpret_cast<void*>(&EnterCriticalSection)) return nullptr;
        auto* result = reinterpret_cast<CRITICAL_SECTION*>(site+7+scope_dlss::Read<int32_t>(site,3));
        Log("[scope-dlss] isolated viewport=%08X native lock RVA=%llX\n",scope_dlss::kViewport,
            static_cast<unsigned long long>(reinterpret_cast<uint8_t*>(result)-g_exe_base));
        return result;
    }();
    return lock;
}
struct NativeDlssGuard {
    CRITICAL_SECTION* lock = NativeDlssLock();
    NativeDlssGuard() { if(lock) EnterCriticalSection(lock); }
    ~NativeDlssGuard() { if(lock) LeaveCriticalSection(lock); }
};
uintptr_t ScopeObject(void* spec) {
    __try {
        if(!spec) return 0;
        const auto ctx=*reinterpret_cast<uintptr_t*>(static_cast<uint8_t*>(spec)+0x18);
        return ctx && *reinterpret_cast<uint64_t*>(ctx+0x28)==cvr::cname_hash("vr_optics_feed")
            ? sl_view_obj(spec) : 0;
    } __except(EXCEPTION_EXECUTE_HANDLER) { return 0; }
}
uint8_t* NativeDlssState() {
    __try {
        const auto renderer=*reinterpret_cast<uintptr_t*>(g_exe_base+RENDERER_GLOBAL_RVA);
        auto* state=renderer ? *reinterpret_cast<uint8_t**>(renderer+OFF_VIEWSTATE) : nullptr;
        if(!state) return nullptr;
        const auto* handle=reinterpret_cast<const sl::ViewportHandle*>(state+0x480);
        if(handle->structType!=sl::ViewportHandle::s_structType || handle->structVersion!=sl::kStructVersion1) {
            static bool warned=false;
            if(!warned) { warned=true; Log("[scope-dlss] native viewport ABI mismatch; isolation disabled\n"); }
            return nullptr;
        }
        return state;
    } __except(EXCEPTION_EXECUTE_HANDLER) { return nullptr; }
}
// The scope RTT's view frame can stay at zero. Use the same renderer clock for
// its constants AND evaluate; restore the view field before leaving the call.
struct ScopeFrameToken {
    uint32_t* field;
    uint32_t saved;
    ScopeFrameToken(uintptr_t obj,uint32_t frame) : field(reinterpret_cast<uint32_t*>(obj+0x1a0)),saved(*field) { *field=frame; }
    ~ScopeFrameToken() { *field=saved; }
};
}

static __int64 __fastcall SlConstantsInner(void* a1, void* a2, void* a3) {
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
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugMainCtxBinds);
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
                    uint32_t* mode = reinterpret_cast<uint32_t*>(obj + 0xFA4);
                    const uint32_t build_mode =
                        *reinterpret_cast<uint32_t*>(obj + 0xFA0);
                    if (key == g_vrcam_ctx_key || key == cvr::cname_hash("vr_optics_feed")) {
                        if(key == g_vrcam_ctx_key) {
                            CyberpunkVR_DebugVrcamAaMode = *mode;
                            CyberpunkVR_DebugVrcamBuildModeF90 = build_mode;
                        }
                        uint32_t want = CyberpunkVR_DebugMainAaMode;   // mirror main
                        if (*mode != want) { *mode = want; CVR_DIAGNOSTIC(++CyberpunkVR_DebugSlHistoryHits); }
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
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugForceCamHits);
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }

    // DLSS-for-vrcam: mark this camera-writer call as vrcam so the constants driver
    // (sub_1407EEA54, called INSIDE g_orig) flips to vrcam's own SL viewport.
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

static __int64 __fastcall Detour_SlConstants(void* a1, void* a2, void* a3) {
    NativeDlssGuard guard;
    const auto obj=ScopeObject(a2);
    auto* state=NativeDlssState();
    uint32_t frame=0;
    if(!guard.lock || !obj || !state || !framegen::ReadRenderFrameIndex(&frame))
        return SlConstantsInner(a1,a2,a3);
    scopeState.Select(reinterpret_cast<uintptr_t>(state),obj);
    scopeState.PrepareFrame(frame);
    scope_dlss::Swap swap(state,scopeState);
    ScopeFrameToken token(obj,frame);
    struct Active {
        bool previous=scopeConstants;
        Active() { scopeConstants=true; }
        ~Active() { scopeConstants=previous; }
    } active;
    return SlConstantsInner(a1,a2,a3);
}

// DLSS constants driver (sub_1407EEA54 -> slSetConstants). Flip to vrcam's SL
// viewport while the camera writer is processing the vrcam view.
static __int64 __fastcall DlssConstInner(void* a1, unsigned int a2) {
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
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugVrcamDlssConstHits);
        } __except (EXCEPTION_EXECUTE_HANDLER) { flipped = false; }
    }
    // Framegen also consumes native TAA: its eye identity must not depend on
    // whether the Streamline viewport override is enabled for DLSS.
    if(!scopeConstants)
        cvr::framegen::RecordNativeCamera(a1,cvr::camera::CurrentNodePoseIdentity().view);
    if(scopeConstants && scopeState.reset)
        *reinterpret_cast<uint8_t*>(static_cast<uint8_t*>(a1)+0x1f0)=0; // native -> Constants.reset
    __int64 r = g_orig_dlss_const(a1, a2);
    if (flipped) {
        __try {
            *reinterpret_cast<int32_t*>(reinterpret_cast<uint8_t*>(a1) + DLSS_VP_OFF) = saved;
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return r;
}

static __int64 __fastcall Detour_DlssConst(void* a1,unsigned int frame) {
    NativeDlssGuard guard;
    const auto result=DlssConstInner(a1,frame);
    if(scopeConstants && a1)
        scopeState.constants=scope_dlss::Read<uint64_t>(static_cast<uint8_t*>(a1),scope_dlss::kToken)==frame;
    return result;
}

// DLSS tag+eval driver (sub_14228F210). a2 = view spec; key at *(*(a2+0x18)+0x28).
// Flip to vrcam's own SL viewport around the whole call (covers its internal
// slSetTag x N + slEvaluateFeature) so vrcam gets a distinct DLSS feature/history.
static void __fastcall DlssEvalInner(void* a1, void* a2, int a3, int a4, int a5,
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
                CVR_DIAGNOSTIC(++CyberpunkVR_DebugVrcamDlssEvalHits);
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

static void __fastcall Detour_DlssEval(void* a1,void* a2,int a3,int a4,int a5,
                                     int a6,int a7,int a8,int a9,int a10,
                                     int a11,int a12,int a13,int a14) {
    NativeDlssGuard guard;
    const auto obj=ScopeObject(a2);
    if(guard.lock && obj && a1 && scopeState.constants && scopeState.view==obj &&
       scopeState.owner==reinterpret_cast<uintptr_t>(a1)) {
        scope_dlss::Swap swap(static_cast<uint8_t*>(a1),scopeState);
        ScopeFrameToken token(obj,scopeState.frame);
        struct Evaluation {
            uint8_t* previous=scopeEvaluation;
            explicit Evaluation(void* state) { scopeEvaluation=static_cast<uint8_t*>(state); }
            ~Evaluation() { scopeEvaluation=previous; }
        } evaluation(a1);
        g_orig_dlss_eval(a1,a2,a3,a4,a5,a6,a7,a8,a9,a10,a11,a12,a13,a14);
        static unsigned reports=0;
        if(reports++<3) {
            auto* p=static_cast<uint8_t*>(a1);
            const auto* handle=reinterpret_cast<const sl::ViewportHandle*>(p+0x480);
            Log("[scope-dlss] evaluate actual_viewport=%08X version=%llu frame=%u input=%ux%u output=%ux%u\n",
                uint32_t(*handle),static_cast<unsigned long long>(handle->structVersion),scopeState.frame,scope_dlss::Read<uint32_t>(p,0x3d0),
                scope_dlss::Read<uint32_t>(p,0x3d4),scope_dlss::Read<uint32_t>(p,0x3e0),scope_dlss::Read<uint32_t>(p,0x3e4));
        }
        return;
    }
    if(obj) {
        static bool warned=false;
        if(!warned) { warned=true; Log("[scope-dlss] no matching scope constants; using native path\n"); }
    }
    DlssEvalInner(a1,a2,a3,a4,a5,a6,a7,a8,a9,a10,a11,a12,a13,a14);
}

// Did MAIN's ApplyDLSS have flag 0x45 set (i.e. is the game actually using DLSS)? Observed
// each frame from main's ApplyDLSS call; vrcam only mirrors it when true. This makes the
// feature a strict MIRROR of main's upscaler: if the player has DLSS OFF (main lacks flag
// 0x45, or the ApplyDLSS node isn't even emitted), vrcam is never forced into DLSS.
static bool g_main_dlss_flag = false;

// ApplyDLSS node work-fn: mirror main's DLSS onto vrcam. For main (key 0) we OBSERVE flag
// 0x45; for vrcam we SET it (only if main has it) so vrcam takes the full eval path AND all
// POST-DLSS nodes (tonemap/bloom sub_140416600 read flags 0x45/0x47/0x48/0x49 to pick the
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
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugVrcamApplyDlssHits);
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
// sub_140425054 computes each view's DLSS render resolution. Its a1 == &view[0x34] (the
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
// sub_140557204, gated by GROUP 20 = bit20 of view+0x17D0 (sub_14033E358(ctx,20)). LIVE-CONFIRMED:
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
extern "C" void CyberpunkVR_GeometryCostTrackViewport(ID3D12GraphicsCommandList*,UINT,const D3D12_VIEWPORT*);
void STDMETHODCALLTYPE hk_RSSetViewports(
        ID3D12GraphicsCommandList* self, UINT count, const D3D12_VIEWPORT* vps) {
    // Pass-through. The post-DLSS crop is fixed natively in Detour_RenderRes (view+0x17D0 match-main);
    // the old render-res-viewport band-aid + per-frame stack-capture diagnostics were removed.
    const CommandListVtableHook* e = command_list_hook_entry(self);
    PFN_RSSetViewports orig = e ? e->viewports_original : nullptr;
    if (orig && !cvr::single_pass::Viewports(self,count,vps)) orig(self, count, vps);
    cvr::gpu::scene_profile::Viewport(self,count,vps);
    CyberpunkVR_GeometryCostTrackViewport(self,count,vps);
    cvr::stereo::pair_probe::Viewports(self,count,vps);
    cvr::vrs::Viewports(self,count,vps);
    if(CyberpunkVR_NativeStereoProbeState.load(std::memory_order_relaxed)==1)cvr::stereo::native_probe::Viewports(self,count,vps);
}
extern "C" void CyberpunkVR_GeometryCostTrackScissor(ID3D12GraphicsCommandList*,UINT,const D3D12_RECT*);
 void STDMETHODCALLTYPE hk_RSSetScissorRects(
        ID3D12GraphicsCommandList* self, UINT count, const D3D12_RECT* rects) {
    // Pass-through (see hk_RSSetViewports).
    const CommandListVtableHook* e = command_list_hook_entry(self);
    PFN_RSSetScissorRects orig = e ? e->scissor_original : nullptr;
    if (orig && !cvr::single_pass::Scissors(self,count,rects)) orig(self, count, rects);
    CyberpunkVR_GeometryCostTrackScissor(self,count,rects);
    if(CyberpunkVR_NativeStereoProbeState.load(std::memory_order_relaxed)==1)cvr::stereo::native_probe::Scissors(self,count,rects);
}
 HRESULT STDMETHODCALLTYPE hk_GfxReset(
        ID3D12GraphicsCommandList* self, ID3D12CommandAllocator* alloc,
        ID3D12PipelineState* pso) {
    if(self->GetType()==D3D12_COMMAND_LIST_TYPE_COPY){
        const auto* entry=command_list_hook_entry(self);if(!entry || !entry->reset_original)return E_FAIL;
        const auto result=entry->reset_original(self,alloc,pso);
        if(SUCCEEDED(result)){
            cvr::list_audit::ListReset(self,alloc);
            cvr::stereo::retained_geometry::Reset(self);
            cvr::stereo::target_access::Reset(self);
            cvr::stereo::texture_read::Reset(self);cvr::stereo::material_binding::Reset(self);
            cvr::stereo::buffer_read::Reset(self);
            cvr::stereo::constants::ResetRegistered(self);cvr::gpu::ResetCommandResources(self);
        }
        return result;
    }
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
    // The outline-swap ordinal counts within ONE recording. Resetting it here is what makes it
    // usable at all: the previous version reset only on a node change, so with the same node
    // recurring all session the ordinal matched exactly once per session.
    vision_swap_list_reset();
    const CommandListVtableHook* e = command_list_hook_entry(self);
    PFN_GfxReset orig = e ? e->reset_original : nullptr;
    const HRESULT result=orig ? orig(self,alloc,pso) : S_OK;
    if(orig && SUCCEEDED(result)) {
        cvr::list_audit::ListReset(self,alloc);
        census_pipeline_reset(pso);
        cvr::stereo::retained_geometry::Reset(self);
        cvr::stereo::native_recording::Reset(self,reinterpret_cast<uintptr_t>(_ReturnAddress()));
        cvr::stereo::target_access::Reset(self);
        cvr::stereo::texture_read::Reset(self);
        cvr::stereo::buffer_read::Reset(self);
        cvr::stereo::material_binding::Reset(self);
        cvr::stereo::source_cbv::Reset(self);
        cvr::stereo::source_srv::Reset(self);
        cvr::stereo::families::Reset(self,pso);
        cvr::stereo::constants::Reset(self);
        cvr::stereo::constants::ResetRegistered(self);
        cvr::stereo::preparation::ResetList(self);
        if(CyberpunkVR_NativeStereoProbeState.load(std::memory_order_relaxed)==1)
            cvr::stereo::native_probe::Reset(self,pso,e->shading_state_known);
        cvr::gpu::ResetCommandResources(self);
        cvr::cascades::Reset(self);
        cvr::gpu::scene_profile::Reset(self);
        cvr::stereo::pair_probe::Reset(self);
        cvr::single_pass::Reset(self);
        cvr::foveated::ResetList(self);
        cvr::vrs::Reset(self,e->shading_rate_original!=nullptr,pso);
        cvr::optics::ListReset(self);cvr::optics::source::ListReset(self);cvr::optics::camera::ListReset(self);
        cvr::camera::ResetImagePoseList(self);
        cvr::framegen::ResetList(self);
        cvr::menu_surface::ResetList(self);
    }
    return result;
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
extern "C" void __fastcall OnDLSSResCallback(void* dlssPtr);   // Hooks/DlssRes.cpp: the size log
namespace {
bool ScopeResolutionSiteMatches() {
    // Installed CP2.5 scaler: upscaler tests followed by native unscaled size writes.
    // Verify beyond the detoured entry, which MinHook has already patched.
    static const bool matches=[] {
        constexpr uint8_t expected[]{
            0x49,0x8b,0x86,0xb8,0x25,0x00,0x00,0xa8,0x40,0x75,0x57,0x84,0xc0,0x78,0x53,0x48,0x0f,0xba,0xe0,0x09,0x72,0x4c,0x48,0x0f,0xba,0xe0,0x08,0x72,0x45,0x48,0x0f,0xba,0xe0,0x0d,0x72,0x3e,0x48,0x0f,0xba,0xe0,0x0c,0x72,0x37,0x48,0x0f,0xba,0xe0,0x0a,0x72,0x30,0x48,0x0f,0xba,0xe0,0x0e,0x72,0x29,0x48,0x0f,0xba,0xe0,0x1b,0x72,0x22,0x44,0x8b,0x5b,0x24,0x45,0x8b,0xc3,0x44,0x8b,0x4b,0x20,0x41,0x8b,0xf3,0x44,0x89,0x5b,0x1c,0x41,0x8b,0xfb,0x44,0x89,0x5b,0x18,0x4c,0x89,0x63,0x30,0xe9,0xdc,0x00,0x00,0x00
        };
        const bool ok=std::memcmp(g_exe_base+RENDER_RES_RVA+0x569,expected,sizeof(expected))==0;
        if(!ok)Log("[scope-resolution] native scaler signature mismatch; full resolution disabled\n");
        return ok;
    }();
    return matches;
}
bool ScopeFullResolutionReady(void* dimensions,void* rect,void* ctx) {
    if(!scope_dlss::fullResolution.load(std::memory_order_relaxed))return false;
    bool eligible=false;
    __try {
        if(!dimensions || !rect || !ctx ||
           dimensions!=static_cast<uint8_t*>(ctx)+0x34)return false;
        const auto* bounds=static_cast<const int32_t*>(rect);
        const auto* view=static_cast<const uint8_t*>(ctx);
        eligible=scope_dlss::FullResolutionEligible(true,
            scope_dlss::Read<uint64_t>(view,0x28)==cvr::cname_hash("vr_optics_feed"),
            scope_dlss::Read<uint64_t>(view,0x25b8),
            int64_t(bounds[2])-bounds[0],int64_t(bounds[3])-bounds[1]);
    } __except(EXCEPTION_EXECUTE_HANDLER){return false;}
    return eligible && originalRrOptions && ScopeResolutionSiteMatches() &&
        NativeDlssLock() && NativeDlssState();
}
__int64 RenderScopeFullResolution(void* dimensions,void* rect,void* ctx) {
    __int64 result;
    {
        auto& flags=*reinterpret_cast<uint64_t*>(static_cast<uint8_t*>(ctx)+0x25b8);
        scope_dlss::NativeResolutionFlags nativeResolution(flags);
        result=g_orig_render_res(dimensions,rect,ctx);
    } // Restore RR/stereo/graph flags BEFORE returning to any builder or pass.
    static std::atomic<unsigned> reports{};
    if(reports.fetch_add(1,std::memory_order_relaxed)<3) {
        const auto* d=static_cast<const uint8_t*>(dimensions);
        Log("[scope-resolution] RR full-size render=%ux%u target=%ux%u; graph flags restored\n",
            scope_dlss::Read<uint32_t>(d,0),scope_dlss::Read<uint32_t>(d,4),
            scope_dlss::Read<uint32_t>(d,0x20),scope_dlss::Read<uint32_t>(d,0x24));
    }
    return result;
}
}
static __int64 __fastcall Detour_RenderRes(void* a1, void* a2, void* a3) {
    if(ScopeFullResolutionReady(a1,a2,a3))return RenderScopeFullResolution(a1,a2,a3);
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
                    *reinterpret_cast<uint64_t*>(view + DLSS_FLAGSET_OFF) &= ~BUILD_FLAG65_BIT;
                    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugClearFlag64Hits)));
                }
                // PLAN B (live-tunable): match MAIN's view+0x17D0 group flags so vrcam SKIPS the
                // raster tonemap (group 20) and composites via the COMPUTE path like main -> no crop.
                if (CyberpunkVR_VrcamComputeResolve) {
                    uint64_t* p = reinterpret_cast<uint64_t*>(view + 0x25B0);   // 2.31: +0x17D0
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
                    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugP17D0Hits)));
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { vrcam = false; }
    }
    __int64 r = g_orig_render_res(a1, a2, a3);
    OnDLSSResCallback(a1);   // what the engine chose, before the vrcam rescale below
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
                // 2.5's writer clamps every dimension to at least 16 (sub_1403DF6C0).
                if (rW < 16) rW = 16;
                if (rH < 16) rH = 16;
                p[0] = rW; p[4] = rW; p[5] = rW;    // renderW: a1+0, a1+16, a1+20
                p[1] = rH; p[6] = rH; p[7] = rH;    // renderH: a1+4, a1+24, a1+28
                *reinterpret_cast<uint64_t*>(reinterpret_cast<uint8_t*>(a1) + 48) = 0; // reset accum (engine scaled branch does this)
                CyberpunkVR_DebugVrcamRenderW = static_cast<uint32_t>(rW);
                CyberpunkVR_DebugVrcamRenderH = static_cast<uint32_t>(rH);
                // publish vrcam render+output dims for the DLSS const/eval subrect+MV fix
                g_vrcam_dlss_rw = rW; g_vrcam_dlss_rh = rH;
                g_vrcam_dlss_ow = tW; g_vrcam_dlss_oh = tH;
                CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugVrcamResScaleHits)));
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return r;
}


// ---- registered here, in the file that defines them --------------------------------------------
CVR_DETOUR("[flagforce] flag-compute sub_14227E5D0", FLAG_COMPUTE_RVA, Detour_FlagCompute, g_orig_flag_compute)
CVR_DETOUR("[dlss] ApplyDLSS work sub_14077E4C8", APPLYDLSS_WORK_RVA, Detour_ApplyDlss, g_orig_applydlss)
CVR_DETOUR("[dlss] constants driver sub_1407EEA54", DLSS_CONST_RVA, Detour_DlssConst, g_orig_dlss_const)
CVR_DETOUR("[dlss] tag/eval driver sub_14228F210", DLSS_EVAL_RVA, Detour_DlssEval, g_orig_dlss_eval)
CVR_DETOUR("[dlss] render-res scaler sub_140425054", RENDER_RES_RVA, Detour_RenderRes, g_orig_render_res)
CVR_DETOUR("[sl] SetStreamlineConstants sub_1407EE0BC", SL_CONSTANTS_RVA, Detour_SlConstants, g_orig_sl_const)
CVR_DETOUR_IF("[scope] RR options sizing", 0x22902d0, ScopeRrOptions, originalRrOptions, RrOptionsSiteMatches)

// ================================================================================================
// THE VRCAM STREAMLINE VIEWPORT KNOBS, moved out of the monolith to sit with the DLSS path they steer.
//
// Giving vrcam its OWN Streamline viewport is what stops the two views sharing one set of DLSS history
// and jitter. These are the live controls for that: which viewport id, whether jitter is zeroed, and
// the render/output dimensions the second view reports.
// ================================================================================================

// ---- DLSS-for-VRCAM: give vrcam its OWN Streamline viewport ----------------
// The engine's DLSS drivers key off ONE global DLSS-state object (arg a1); the
// Streamline ViewportHandle is at a1+0x480 with the viewport id at a1+0x4A0 (=0
// for main). Both the constants driver (sub_1407EEA54 -> slSetConstants) and the
// tag+eval driver (sub_14228F210 -> slSetTag x N + slEvaluateFeature) submit to
// that single viewport, so vrcam (which already reaches the eval driver with its
// OWN ctx + resources -- verified live: eval a2 key == VRCAM) collides on main's
// viewport 0 and produces no distinct DLSS. Fix: when the current view is vrcam,
// flip a1+0x4A0 to a distinct id (default 1) around each driver call and restore
// after (idempotent, no persistent state). Streamline auto-creates a 2nd DLSS
// feature (own temporal history) for the new viewport on first eval. RVAs @ base
// 0x7FF6EF660000 (found via named refs to sl.interposer exports).
using DlssEvalFn  = void(__fastcall*)(void*, void*, int, int, int, int, int,
                                      int, int, int, int, int, int, int);
using DlssConstFn = __int64(__fastcall*)(void*, unsigned int);
DlssEvalFn  g_orig_dlss_eval  = nullptr;
DlssConstFn g_orig_dlss_const = nullptr;
// PATH-A graph-level EXPERIMENT (doc 19): the DLSS-gated frame-graph node
// sub_142E9E3EC (vtable 0x14312CDA8 +0x28) inserts an extra type-4 WRITE declare of
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
// group 69 set, so the player-facing switch was only ever the redundant half of an AND -- and the
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

// ApplyDLSS node work-fn (sub_14077E4C8). vrcam takes a NO-EVAL path because it lacks
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
// FIX: hook the getter ENTRY sub_1403DFEA4 (clean 5-byte prologue, unlike the tiny-leaf
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

// The eval driver's "feature changed?" check (in sub_14228F210) compares the CURRENT view
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
